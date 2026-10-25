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
# 5. The delegated run transcript must be a declared field on the run DTO
#    AND be populated by production code. A field that is declared but
#    never populated is a payload that crosses as nothing while the
#    contract claims it crosses.

set -euo pipefail

ROOT=${CODEGG_GUARD_ROOT:-$(git rev-parse --show-toplevel)}
REPLAY_DIR="$ROOT/crates/codegg-core/src/projection_replay"
DTO="$ROOT/crates/codegg-protocol/src/projection/dto.rs"
violations=0

# The self-test builds a miniature tree and runs this same guard against
# it, so the transcript rule is proved to fire on the violation it claims
# to catch rather than merely being present in the file.
run_self_test() {
  SELF_TEST_FIXTURE=$(mktemp -d)
  local fixture="$SELF_TEST_FIXTURE"
  mkdir -p "$fixture/crates/codegg-protocol/src/projection" \
    "$fixture/crates/codegg-core/src/projection_replay" \
    "$fixture/src"
  cat > "$fixture/crates/codegg-protocol/src/projection/dto.rs" <<'EOF'
pub struct AgentRunSummary {
    pub transcript: Option<AgentRunTranscriptProjection>,
}
EOF
  cat > "$fixture/crates/codegg-core/src/projection_replay/agent_runs.rs" <<'EOF'
pub fn populate(summary: &mut AgentRunSummary) {
    summary.transcript = Some(AgentRunTranscriptProjection::Absent);
}
EOF
  if ! CODEGG_GUARD_ROOT="$fixture" "$0" >/dev/null 2>&1; then
    echo "ERROR: self-test — the guard rejected a populated transcript field."
    return 1
  fi
  echo "OK: self-test — a declared and populated transcript field passes."
  # The injected violation: the field is declared and nothing populates it.
  cat > "$fixture/crates/codegg-core/src/projection_replay/agent_runs.rs" <<'EOF'
pub fn populate(summary: &mut AgentRunSummary) {
    let _ = summary;
}
EOF
  if CODEGG_GUARD_ROOT="$fixture" "$0" >/dev/null 2>&1; then
    echo "ERROR: self-test — the guard accepted a transcript field that is declared but never populated."
    return 1
  fi
  echo "OK: self-test — a declared but unpopulated transcript field is rejected."
  rm -rf "$fixture"
  return 0
}

if [ "${1:-}" = "--self-test" ]; then
  run_self_test
  exit $?
fi

# ── 5. Declared-and-populated delegated run transcript ────────────────
# The field's declaration.
if ! grep -q "pub transcript: Option<AgentRunTranscriptProjection>" "$DTO" 2>/dev/null; then
  echo "ERROR: $DTO does not declare the delegated run transcript field."
  echo "  The transcript payload must be a declared field on the run DTO."
  violations=1
fi
# Its population. The protocol crate may only *declare* it; a production
# site outside the protocol crate must put a payload on a run summary.
populate_matches=$(grep -rEn "\.transcript[[:space:]]*=[[:space:]]*Some\(" \
  --include='*.rs' "$ROOT/src" "$ROOT/crates/codegg-core/src" 2>/dev/null || true)
if [ -z "$populate_matches" ]; then
  echo "ERROR: no production site populates the delegated run transcript field."
  echo "  The field is declared but nothing puts a transcript on a run summary,"
  echo "  so the tab would render an absent transcript for every run."
  violations=1
fi

# ── 1. SafePublicationClass variant usage outside replay dir ──────────
# SafePublicationClass is pub(crate) but the guard checks that
# SafePublicationClass::Internal / Sensitive are not referenced
# outside the replay module boundary.
for variant in "SafePublicationClass::Internal" "SafePublicationClass::Sensitive"; do
  matches=$(grep -rEn "$variant" --include='*.rs' "$ROOT/src" "$ROOT/crates" 2>/dev/null || true)
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
path_matches=$(grep -rEn 'source_record_id[[:space:]]*:[[:space:]]*"' \
  --include='*.rs' "$ROOT/src" "$ROOT/crates" 2>/dev/null || true)
if [ -n "$path_matches" ]; then
  while IFS= read -r line; do
    file=$(echo "$line" | cut -d: -f1)
    lineno=$(echo "$line" | cut -d: -f2)
    content=$(echo "$line" | cut -d: -f3-)
    # Check if the value starts with / or contains ..
    if echo "$content" | grep -qE 'source_record_id[[:space:]]*:[[:space:]]*"/' 2>/dev/null; then
      echo "ERROR: $file:$lineno — source_record_id starts with '/'"
      echo "  ProjectionArtifactHandle source_record_id must not be a filesystem path."
      violations=1
    fi
    if echo "$content" | grep -qE 'source_record_id[[:space:]]*:[[:space:]]*"[^"]*\.\.' 2>/dev/null; then
      echo "ERROR: $file:$lineno — source_record_id contains '..'"
      echo "  ProjectionArtifactHandle source_record_id must not contain path traversal."
      violations=1
    fi
  done <<< "$path_matches"
fi

# ── 3. HandleRegistry::mint_id outside replay dir ────────────────────
mint_matches=$(grep -rEn "HandleRegistry::mint_id" --include='*.rs' \
  "$ROOT/src" "$ROOT/crates" 2>/dev/null || true)
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
oversized_matches=$(grep -rEn '\[REDACTED:oversized:' --include='*.rs' \
  "$ROOT/src" "$ROOT/crates" 2>/dev/null || true)
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
