#!/usr/bin/env bash
# Static guard: enforce M3 disclosure pipeline encapsulation invariants.
#
# 1. Forbid direct use of SafePublicationClass::Public/Internal/Sensitive
#    from outside crates/codegg-core/src/projection_replay/ (these must
#    be encapsulated by the disclosure policy).
# 2. Forbid direct instantiation of ProjectionArtifactHandle with a
#    source_record_id starting with '/' or containing '..'.
# 3. Forbid use of HandleRegistry::mint_id outside
#    crates/codegg-core/src/projection_replay/ — only
#    HandleRegistrar::mint is the public API.
# 4. Forbid the literal string '[REDACTED:oversized:' outside
#    crates/codegg-core/src/projection_replay/redactor.rs (the
#    downgraded marker must not be synthesized by callers).

set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
REPLAY_DIR="$ROOT/crates/codegg-core/src/projection_replay"
violations=0

# ── 1. SafePublicationClass variant usage outside replay dir ──────────
# SafePublicationClass is pub(crate) but the guard checks that
# SafePublicationClass::Internal / Sensitive are not referenced
# outside the replay module boundary.
for variant in "SafePublicationClass::Internal" "SafePublicationClass::Sensitive"; do
  matches=$(rg -n --type rust "$variant" "$ROOT/src" "$ROOT/crates" 2>/dev/null || true)
  if [ -n "$matches" ]; then
    while IFS= read -r line; do
      file=$(echo "$line" | cut -d: -f1)
      # Allow within the projection_replay directory and tests
      if [[ "$file" == "$REPLAY_DIR"* ]] || [[ "$file" == "$ROOT/tests"* ]]; then
        continue
      fi
      echo "ERROR: $line"
      echo "  SafePublicationClass variants must be encapsulated by the disclosure policy."
      echo "  Only projection_replay modules and tests may reference them directly."
      violations=1
    done <<< "$matches"
  fi
done

# ── 2. Path-unsafe source_record_id in ProjectionArtifactHandle ──────
# Look for struct literal construction with source_record_id: "/" or ".."
path_matches=$(rg -n --type rust 'source_record_id\s*:\s*"' "$ROOT/src" "$ROOT/crates" 2>/dev/null || true)
if [ -n "$path_matches" ]; then
  while IFS= read -r line; do
    file=$(echo "$line" | cut -d: -f1)
    lineno=$(echo "$line" | cut -d: -f2)
    content=$(echo "$line" | cut -d: -f3-)
    # Check if the value starts with / or contains ..
    if echo "$content" | rg -q 'source_record_id\s*:\s*"/' 2>/dev/null; then
      echo "ERROR: $file:$lineno — source_record_id starts with '/'"
      echo "  ProjectionArtifactHandle source_record_id must not be a filesystem path."
      violations=1
    fi
    if echo "$content" | rg -q 'source_record_id\s*:\s*"[^"]*\.\.' 2>/dev/null; then
      echo "ERROR: $file:$lineno — source_record_id contains '..'"
      echo "  ProjectionArtifactHandle source_record_id must not contain path traversal."
      violations=1
    fi
  done <<< "$path_matches"
fi

# ── 3. HandleRegistry::mint_id outside replay dir ────────────────────
mint_matches=$(rg -n --type rust "HandleRegistry::mint_id" "$ROOT/src" "$ROOT/crates" 2>/dev/null || true)
if [ -n "$mint_matches" ]; then
  while IFS= read -r line; do
    file=$(echo "$line" | cut -d: -f1)
    if [[ "$file" == "$REPLAY_DIR"* ]] || [[ "$file" == "$ROOT/tests"* ]]; then
      continue
    fi
    echo "ERROR: $line"
    echo "  HandleRegistry::mint_id is internal API; use HandleRegistrar::mint instead."
    violations=1
  done <<< "$mint_matches"
fi

# ── 4. Oversized marker outside redactor.rs ──────────────────────────
oversized_matches=$(rg -n --type rust '\[REDACTED:oversized:' "$ROOT/src" "$ROOT/crates" 2>/dev/null || true)
if [ -n "$oversized_matches" ]; then
  while IFS= read -r line; do
    file=$(echo "$line" | cut -d: -f1)
    # Only redactor.rs may synthesize this marker
    if [[ "$file" == "$REPLAY_DIR/redactor.rs" ]]; then
      continue
    fi
    echo "ERROR: $line"
    echo "  '[REDACTED:oversized:' marker must only be synthesized by redactor.rs."
    violations=1
  done <<< "$oversized_matches"
fi

# ── Result ────────────────────────────────────────────────────────────
if [ "$violations" -ne 0 ]; then
  echo ""
  echo "FAILED: projection disclosure guard found violations."
  exit 1
fi

echo "OK: no projection disclosure encapsulation violations."
