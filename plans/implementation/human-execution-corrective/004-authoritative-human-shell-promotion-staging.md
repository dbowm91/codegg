# Human Execution Corrective C004 — Authoritative Human-Shell Promotion Staging

Status: ready for handoff

Repository baseline: `2f026f05220f60c62bfc381af3d7479172dfdb50`

Source roadmap:

- `plans/subsystems/human-execution-corrective-roadmap.md#c001--promotion-redaction-and-config-truth`

Corrective for:

- `plans/implementation/human-execution-corrective/001-human-shell-promotion-and-redaction.md`
- `plans/closure/human-execution-corrective/001-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: none identified. If session/context storage cannot safely own bounded pending promotions under existing authorization, stop and request a specific architecture decision rather than inventing a generic context store.

Primary class: invariant/capability/infrastructure.

## 1. Objective

Move pending human-shell promotion ownership from frontend memory to the authoritative session/context owner. `!!command` and `/shell-include` must receive a typed acknowledgement when a bounded/redacted projection is staged for the exact session. A subsequent accepted turn consumes it once. `/shell-ask` must stage and submit atomically or use an equivalent idempotent session-owner operation. Pending state must recover across TUI reconnect/restart under a finite retention contract.

## 2. Why this corrective is required

C001 established a real provider-facing turn path and redaction tests, but its pending include queue remains in TUI memory until a turn is submitted. Turn acknowledgement proves consumption, not canonical staging. No authoritative staging identity survives frontend loss, and a remote composer insertion has no distinct staging acknowledgement. Closure `001-status.md` records this as a correctness gap. C002 remains blocked until C001 and this corrective close.

## 3. Current implementation evidence

`src/tui/app/mod.rs` holds pending promotion tokens and `src/tui/app/prompt_turn.rs` attaches them to a submitted turn. `src/tui/commands/shell.rs` builds the redacted projection and fences originating session/workspace. The canonical turn and ask submissions accept model-facing messages, but there is no durable stage-before-turn request, authoritative pending record, reconnect query, expiry state, or dedupe identity at the session owner.

## 4. Invariants that must not regress

- `!` and raw shell cells never enter provider context.
- Human approval is explicit, authenticated, session-bound, and cannot be forged from a client-supplied origin field.
- Only the canonical redacted/model-target projection is persisted or consumed; raw output remains in its governed shell-output owner.
- A staged record is bounded, idempotent, and consumed by at most one accepted user turn.
- Rejected or busy turn admission does not consume staged evidence.
- Expiry, stale session/workspace, unsupported daemon capability, or restart loss returns an explicit typed result; no false staged/promoted UI state.
- C002's workspace-owned shell execution is not pulled into this correction.

## 5. Scope

### In scope

- Audit existing session/context storage, turn admission, `SessionPromptSubmit`, protocol capability negotiation, and authorization for a narrow staged-promotion lifecycle.
- Add a typed stage/query/consume contract owned by the authoritative session/context layer, reusing current persistence where safe.
- Add stable promotion identity and idempotent retry semantics, exact session binding, bounded retention/expiry, and typed refusal.
- Convert local TUI and WebSocket composer paths to observe canonical acknowledgement and state.
- Test staging, successful consumption, busy/rejected turn, duplicate retries, reconnect/restart, expiry, stale sessions/workspaces, observer denial, and old-daemon capability behavior.
- Reconcile docs, skills, architecture, plan status, and registry.

### Explicitly out of scope

- Moving finite shell execution to the daemon or workspace node (C002).
- Changing PTY lifecycle or screen rendering (C003).
- General-purpose context storage, raw shell transcript persistence, automatic inference on stage, or model-driven shell execution.

## 6. Required production changes

### Core/domain

Define a promotion record with opaque idempotency identity, shell command/run source identity, authenticated human principal, exact session/project/workspace binding, redacted projection bytes, byte count, source completion/truncation metadata, creation/expiry and consumed-turn identity. Validate limits and reject malformed/unsupported versions.

### Storage and migrations

Reuse authoritative session storage where ownership and migration semantics are sound. Otherwise add one additive migration for bounded redacted pending payloads and lifecycle state. Define finite TTL/retention and cleanup behavior. Never persist raw stdout/stderr for this feature. Migration restart/rollback and old-session behavior must be tested.

### Protocol and DTOs

Audit current DTOs and capability handshake before adding a request. Stage must derive human principal and session/workspace authority from authenticated server-side context. Return typed staged, duplicate, expired, unavailable, unauthorized, stale-binding, and unsupported-capability outcomes. Do not treat client-supplied session/workspace as authority.

### Runtime and concurrency

Stage is idempotent by promotion identity. Turn admission and consumption must be atomic or have a recoverable lease/transaction protocol that prevents both duplicate inclusion and evidence loss. Ask must combine question and evidence in one accepted turn. Rejected/busy admission preserves pending state. Reconnect lists only the caller's session-scoped bounded state. Expiry and stale generation cannot attach to a new tab/session.

### Frontend or operator surface

Distinguish projection prepared, server-acknowledged staged, consumed, expired, refused, and unavailable. Preserve pending state after a reconnect by querying the owner. Remote include must not report accepted based only on composer insertion; either stage canonically or clearly identify composer text as an unsubmitted draft and show no staged status.

### Security and authorization

Enforce human origin from authenticated connection authority, observer denial, exact session/workspace binding, cross-session isolation, output bounds, and existing mandatory redaction. Keep projected content untrusted user evidence. Add audit events without secrets/raw output.

### Documentation and static guards

Update `architecture/human_shell.md`, `architecture/session.md` or the actual authoritative owner doc, `architecture/tui.md`, relevant skills, `docs/tui.md`, and `AGENTS.md`. Add a focused guard only if a source-level invariant can prevent recurrence without a new CI lane.

## 7. Ordered work packages

### Work package A — Ownership and API audit

Trace authoritative context persistence, request authentication, admission transactions, capabilities and migrations. Produce a state transition/compatibility matrix before changing DTOs.

### Work package B — Durable stage and idempotent consume

Implement narrow session-owned storage/protocol or prove an existing owner can carry the contract. Add retry-safe consumption coupled to accepted turn admission.

### Work package C — Frontend state and remote semantics

Replace local-only stage truth with canonical acknowledgement/query, truthful UI states and explicit unsupported behavior.

### Work package D — End-to-end recovery qualification

Use deterministic fake provider and daemon/client fixtures to prove exactly-once provider-facing content, reconnect/restart recovery, expiry and denials. Update closure/roadmap/registry only when all required evidence is green.

## 8. Failure, cancellation, restart, and contention semantics

Define transaction boundaries for stage and turn admission; test concurrent duplicate stage and consume attempts; rejected/busy/cancelled turn must not lose pending payload; restart recovers unexpired records; expired records return explicit gone; stale session/workspace and unauthorized requests cannot observe or consume another session's promotion. Keep queue count, payload bytes, TTL and response sizes bounded.

## 9. Compatibility and migration

Older daemons must return an explicit unsupported capability without marking output staged. Additive migration only. Existing session histories remain readable, and rollback must not reinterpret raw shell output as model context.

## 10. Verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib shell
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
# Add the focused daemon/session staging recovery integration target.
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
scripts/verify.sh quick
```

## 11. Documentation updates

Document the canonical staged-state lifecycle, finite expiry, reconnect recovery, unsupported-daemon behavior, and the exact difference between a composer draft and acknowledged promotion. Correct architecture/skill/user guide claims together.

## 12. Acceptance criteria

A staged promotion is durably acknowledged by the authoritative owner, remains visible after client reconnect/restart until consumed or expired, appears exactly once in the intended accepted provider-facing turn, and is never lost on rejected admission. Ask submits one question turn with evidence. Unauthorized, stale, observer, oversized, expired, and older-daemon cases fail explicitly. No shell raw output is persisted or exposed through this API.

## 13. Stop conditions

Stop for an ADR if current storage ownership cannot safely guarantee authenticated session-scoped promotion. Stop on any secret-redaction bypass or non-atomic loss/duplication risk. Do not unblock C002 based on frontend-only staging or a turn-local test.

## 14. Closure evidence

Migration/restart evidence if a schema change is used; request identity and auth trace; state transition matrix; fake-provider exactly-once capture; busy/rejected/duplicate/reconnect/restart/expiry tests; local and remote UX acknowledgement; compatibility result against an old daemon; full quick verification; documentation and registry diffs.
