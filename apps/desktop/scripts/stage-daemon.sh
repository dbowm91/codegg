#!/usr/bin/env bash
set -euo pipefail
app_root="$(cd "$(dirname "$0")/.." && pwd)"
repo_root="$(cd "$app_root/../.." && pwd)"
source_bin="${CODEGG_DAEMON_EXECUTABLE:-$repo_root/target/debug/codegg}"
if [[ ! -x "$source_bin" ]]; then
  echo "CodeGG executable not found or not executable: $source_bin" >&2
  echo 'Build the root codegg binary or set CODEGG_DAEMON_EXECUTABLE.' >&2
  exit 1
fi
dest="$app_root/src-tauri/binaries/codegg"
mkdir -p "$(dirname "$dest")"
install -m 0755 "$source_bin" "$dest"
echo "staged existing codegg executable at $dest"
