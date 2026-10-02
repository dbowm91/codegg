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
# C003 built-app E2E test-only plugin boundary. The embedded WebDriver server
# must be feature-gated and absent from ordinary production builds.
desktop_manifest="$root/apps/desktop/src-tauri/Cargo.toml"
desktop_src="$root/apps/desktop/src-tauri/src"
if ! rg -q '^\s*desktop-e2e\s*=\s*\["tauri-plugin-wdio-webdriver"\]' "$desktop_manifest"; then
  echo 'desktop-e2e feature must enable exactly tauri-plugin-wdio-webdriver' >&2
  exit 1
fi
if rg -n 'tauri-plugin-wdio-webdriver\s*=' "$desktop_manifest" | rg -v '^\s*[0-9]+:desktop-e2e\s*=' | rg -vq 'optional\s*=\s*true'; then
  echo 'tauri-plugin-wdio-webdriver must be an optional dependency' >&2
  exit 1
fi
if rg -n --pcre2 'tauri-plugin-wdio(?!-webdriver)' "$desktop_manifest" "$root/apps/desktop/src-tauri/Cargo.lock" 2>/dev/null; then
  echo 'tauri-plugin-wdio (backend execute/mock/log privileges) requires a documented C003 stop/review' >&2
  exit 1
fi
if rg -n --pcre2 'tauri_plugin_wdio(?!_webdriver)' "$desktop_src" 2>/dev/null; then
  echo 'tauri-plugin-wdio (backend execute/mock/log privileges) requires a documented C003 stop/review' >&2
  exit 1
fi
if rg -ni 'wdio' "$root/apps/desktop/src-tauri/tauri.conf.json" "$capability"; then
  echo 'production Tauri config/capability must not reference the test-only WebDriver plugin' >&2
  exit 1
fi
if [[ -e "$root/apps/desktop/src-tauri/capabilities/e2e.json" ]]; then
  echo 'generated test-only capability src-tauri/capabilities/e2e.json must not exist in a production-clean tree (run apps/desktop/e2e/build-e2e-app.sh clean)' >&2
  exit 1
fi
if [[ ! -f "$root/apps/desktop/e2e/capabilities/e2e.json" ]]; then
  echo 'test-only capability template apps/desktop/e2e/capabilities/e2e.json is missing' >&2
  exit 1
fi
python3 - "$desktop_manifest" "$desktop_src" <<'PY'
import re
import sys
from pathlib import Path

manifest = Path(sys.argv[1]).read_text(encoding="utf-8")
assert re.search(
    r'^desktop-e2e\s*=\s*\["tauri-plugin-wdio-webdriver"\]',
    manifest,
    re.MULTILINE,
), "desktop-e2e feature must map to the test-only plugin"
src = Path(sys.argv[2])
hits = 0
for path in sorted((*src.glob("*.rs"), *src.glob("bin/*.rs"))):
    lines = path.read_text(encoding="utf-8").splitlines()
    for index, line in enumerate(lines):
        if "tauri_plugin_wdio" not in line or line.lstrip().startswith("//"):
            continue
        hits += 1
        window = "\n".join(lines[max(0, index - 3) : index + 1])
        assert 'cfg(feature = "desktop-e2e")' in window, (
            f"{path}:{index + 1} enables the test-only plugin without the "
            'desktop-e2e feature gate'
        )
assert hits >= 1, "expected one feature-gated test-only plugin registration"
PY
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
