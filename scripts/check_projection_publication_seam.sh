#!/usr/bin/env bash
# Static guard: forbid new direct ProjectionReplayHandle::publish_core_event call sites
# outside the centralized EventLog sink. The seam is owned by EventLog.
#
# The delegated run transcript has its own routing rule, checked here as
# well: the transcript reaches the client through the **session**-scoped
# projection snapshot, and it has exactly one producer. A second producer
# is a second routing decision, and a producer outside the session
# snapshot builder would put the transcript on a stream that is not the
# owning session's.
#
# The M002 transcript-advanced ADVISORY has the same rule for the same
# reason: it is published from exactly one module, and it reaches the
# projection stream through the daemon's canonical EventLog — never
# through a direct replay-handle call, and never from a second producer.

set -euo pipefail

ROOT=${CODEGG_GUARD_ROOT:-$(git rev-parse --show-toplevel)}
ALLOWLIST=(
  "$ROOT/src/core/event_log.rs"
  "$ROOT/crates/codegg-core/src/projection_replay/handle.rs"
  "$ROOT/crates/codegg-core/src/projection_replay/seam.rs"
  "$ROOT/tests"
)

# The producer is the session-scoped snapshot builder. The adapter that
# populates the field lives with the other projection adapters.
PRODUCER="src/core/daemon_refresh.rs"
ADAPTER="crates/codegg-core/src/projection_replay/agent_runs.rs"

# The self-test builds a miniature tree and runs this same guard against
# it, so the transcript routing rule is proved to fire on the violations
# it claims to catch rather than merely being present in the file.
run_self_test() {
  SELF_TEST_FIXTURE=$(mktemp -d)
  local fixture="$SELF_TEST_FIXTURE"
  mkdir -p "$fixture/src/core" "$fixture/crates/codegg-core/src/projection_replay"
  cat > "$fixture/$PRODUCER" <<'EOF'
pub async fn projection_snapshot_for_session(&self) {
    let summary = agent_run_summary_with_transcript(summary);
}
EOF
  cat > "$fixture/$ADAPTER" <<'EOF'
pub async fn agent_run_summary_with_transcript(summary: AgentRunSummary) -> AgentRunSummary {
    summary
}
EOF
  mkdir -p "$fixture/src/agent"
  cat > "$fixture/src/agent/transcript_publisher.rs" <<'EOF'
pub async fn publish(&self, event: CoreEvent) {
    let event = CoreEvent::AgentRunTranscriptAdvanced { run_id: "r".into() };
    self.event_log.publish(Some("s".into()), None, event).await;
}
EOF
  if ! CODEGG_GUARD_ROOT="$fixture" "$0" >/dev/null 2>&1; then
    echo "ERROR: self-test — the guard rejected a correctly routed transcript."
    return 1
  fi
  echo "OK: self-test — a transcript routed through the session snapshot builder passes."

  # The injected violation: a second producer of the M002 advisory. The
  # advisory reaches the projection stream through the event log, and one
  # module owns that decision.
  cat > "$fixture/src/core/daemon_other.rs" <<'EOF'
pub async fn elsewhere(&self, event: CoreEvent) {
    let _ = CoreEvent::AgentRunTranscriptAdvanced { run_id: "r".into() };
}
EOF
  if CODEGG_GUARD_ROOT="$fixture" "$0" >/dev/null 2>&1; then
    echo "ERROR: self-test — the guard accepted a second transcript-advance producer."
    return 1
  fi
  echo "OK: self-test — a second transcript-advance producer is rejected."
  rm -f "$fixture/src/core/daemon_other.rs"

  # The injected violation: the advisory no longer reaches the event log.
  cat > "$fixture/src/agent/transcript_publisher.rs" <<'EOF'
pub async fn publish(&self, event: CoreEvent) {
    let _ = CoreEvent::AgentRunTranscriptAdvanced { run_id: "r".into() };
}
EOF
  if CODEGG_GUARD_ROOT="$fixture" "$0" >/dev/null 2>&1; then
    echo "ERROR: self-test — the guard accepted a transcript-advance publisher that bypasses the event log."
    return 1
  fi
  echo "OK: self-test — a transcript-advance publisher that bypasses the event log is rejected."
  cat > "$fixture/src/agent/transcript_publisher.rs" <<'EOF'
pub async fn publish(&self, event: CoreEvent) {
    let event = CoreEvent::AgentRunTranscriptAdvanced { run_id: "r".into() };
    self.event_log.publish(Some("s".into()), None, event).await;
}
EOF

  # The injected violation: a second producer is a second routing decision.
  cat > "$fixture/src/core/daemon_other.rs" <<'EOF'
pub async fn somewhere_else(&self) {
    let summary = agent_run_summary_with_transcript(summary);
}
EOF
  if CODEGG_GUARD_ROOT="$fixture" "$0" >/dev/null 2>&1; then
    echo "ERROR: self-test — the guard accepted a second transcript producer."
    return 1
  fi
  echo "OK: self-test — a second transcript producer is rejected."
  rm -f "$fixture/src/core/daemon_other.rs"

  # The injected violation: the adapter no longer populates the field.
  cat > "$fixture/$ADAPTER" <<'EOF'
pub async fn agent_run_summary_with_transcript(summary: AgentRunSummary) -> AgentRunSummary {
    summary
}
EOF
  if ! CODEGG_GUARD_ROOT="$fixture" "$0" >/dev/null 2>&1; then
    echo "ERROR: self-test — the guard accepted a tree whose producer is missing."
    return 1
  fi
  echo "OK: self-test — a missing transcript producer is rejected."
  rm -rf "$fixture"
  return 0
}

if [ "${1:-}" = "--self-test" ]; then
  run_self_test
  exit $?
fi

violations=0
publication_sites=$(grep -rlE "ProjectionReplayHandle::publish_core_event" \
  --include='*.rs' "$ROOT/src" "$ROOT/crates" 2>/dev/null || true)

if [ -n "$publication_sites" ]; then
  for f in $publication_sites; do
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
      violations=1
    fi
  done
fi

# ── The delegated run transcript's routing ─────────────────────────────
producers=$(grep -rlE "agent_run_summary_with_transcript" --include='*.rs' \
  "$ROOT/src" 2>/dev/null || true)
if [ -z "$producers" ]; then
  echo "ERROR: no production site populates the delegated run transcript."
  echo "  The transcript must be attached by the session projection snapshot builder."
  violations=1
else
  for f in $producers; do
    rel=${f#"$ROOT/"}
    if [ "$rel" != "$PRODUCER" ]; then
      echo "ERROR: $rel populates the delegated run transcript."
      echo "  The transcript crosses on the session projection snapshot, and only"
      echo "  $PRODUCER builds that snapshot. A second producer is a second"
      echo "  routing decision."
      violations=1
    fi
  done
  if ! grep -q "projection_snapshot_for_session" "$ROOT/$PRODUCER" 2>/dev/null; then
    echo "ERROR: $PRODUCER does not define projection_snapshot_for_session."
    echo "  The transcript must be attached by the session-scoped snapshot builder."
    violations=1
  fi
fi

if ! grep -q "agent_run_summary_with_transcript" "$ROOT/$ADAPTER" 2>/dev/null; then
  echo "ERROR: $ADAPTER does not populate the delegated run transcript field."
  echo "  The projection adapter is the populator; the daemon only supplies it."
  violations=1
fi

# ── The M002 transcript-advanced advisory's publication ───────────────
ADVANCE_PRODUCER="src/agent/transcript_publisher.rs"
advance_producers=$(grep -rlE "CoreEvent::AgentRunTranscriptAdvanced" --include='*.rs' \
  "$ROOT/src" 2>/dev/null || true)
if [ -z "$advance_producers" ]; then
  echo "ERROR: no production site publishes the transcript-advanced advisory."
  echo "  The advisory is how a delegated run's transcript growth is signalled."
  violations=1
else
  for f in $advance_producers; do
    rel=${f#"$ROOT/"}
    if [ "$rel" != "$ADVANCE_PRODUCER" ]; then
      echo "ERROR: $rel publishes the transcript-advanced advisory."
      echo "  The advisory must be published from $ADVANCE_PRODUCER, through the"
      echo "  daemon's canonical event log. A second producer is a second"
      echo "  routing decision."
      violations=1
    fi
  done
  if ! grep -q "event_log.publish(" "$ROOT/$ADVANCE_PRODUCER" 2>/dev/null; then
    echo "ERROR: $ADVANCE_PRODUCER does not publish through the event log."
    echo "  The event log is the only canonical producer; a direct publication"
    echo "  would bypass the projection seam."
    violations=1
  fi
fi

# ── Result ────────────────────────────────────────────────────────────
if [ "$violations" -ne 0 ]; then
  echo ""
  echo "FAILED: projection publication seam guard found violations."
  exit 1
fi

echo "OK: no unauthorized direct projection replay publication sites."
echo "OK: the delegated run transcript crosses only on the session projection snapshot."
