#!/usr/bin/env bash
# Static guard: forbid new direct ProjectionReplayHandle::publish_core_event call sites
# outside the centralized EventLog sink. The seam is owned by EventLog.

set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
ALLOWLIST=(
  "$ROOT/src/core/event_log.rs"
  "$ROOT/crates/codegg-core/src/projection_replay/handle.rs"
  "$ROOT/crates/codegg-core/src/projection_replay/seam.rs"
  "$ROOT/tests"
)

violations=$(rg -l --type rust "ProjectionReplayHandle::publish_core_event" "$ROOT/src" "$ROOT/crates" || true)

if [ -n "$violations" ]; then
  for f in $violations; do
    rel=${f#"$ROOT/"}
    allowed=false
    for a in "${ALLOWLIST[@]}"; do
      if [ "$f" = "${a#"$ROOT/"}" ] || [[ "$rel" == tests/* ]]; then
        allowed=true
        break
      fi
    done
    if [ "$allowed" = false ]; then
      echo "ERROR: $rel calls ProjectionReplayHandle::publish_core_event directly."
      echo "  All publication must route through EventLog's projection sink."
      exit 1
    fi
  done
fi

echo "OK: no unauthorized direct projection replay publication sites."
