# Human Execution Corrective C004 — Authoritative Human-Shell Promotion Staging

Status: blocked (architecture decision pending in `plans/adrs/ADR-0017-human-shell-promotion-admission.md`)

Repository baseline: `d32e1ff28ead2527fd88220a3954dee0d9590bb0` (active work began after M004's hosted-only blocker record)

Source roadmap:

- `plans/subsystems/human-execution-corrective-roadmap.md#c001--promotion-redaction-and-config-truth`

Corrective for:

- `plans/implementation/human-execution-corrective/001-human-shell-promotion-and-redaction.md`
- `plans/closure/human-execution-corrective/001-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADR: proposed `plans/adrs/ADR-0017-human-shell-promotion-admission.md`. If session/context storage cannot safely own bounded pending promotions under existing authorization, request an architecture decision rather than inventing a generic context store.

Primary class: invariant/capability/infrastructure.

## 1. Objective

Move pending human-shell promotion ownership from frontend memory to the authoritative session/context owner. `!!command` and `/shell-include` must receive a typed acknowledgement when a bounded/redacted projection is staged for the exact session. A subsequent accepted turn consumes it once. `/shell-ask` must stage and submit atomically or use an equivalent idempotent session-owner operation. Pending state must recover across TUI reconnect/restart under a finite retention contract.

## 2. Why this corrective is required

C001 established a real provider-facing turn path and redaction tests, but its pending include queue remains in TUI memory until a turn is submitted. Turn acknowledgement proves consumption, not canonical staging. No authoritative staging identity survives frontend loss, and a remote composer insertion has no distinct staging acknowledgement. Closure `001-status.md` records this as a correctness gap. C002 remains blocked until C001 and this corrective close.

## 3. Current implementation evidence

`src/tui/app/mod.rs` holds pending promotion tokens and `src/tui/app/prompt_turn.rs` attaches projected text to the submitted `TurnSubmit.messages`; a successful `CoreResponse::Ack` then marks the local shell entry promoted and drops the local token. `src/tui/commands/shell.rs` builds the bounded redacted projection and fences the captured session/workspace, but `add_user_message` is the staging operation for the local path. The remote include path only inserts an unsubmitted composer draft. `SessionPromptSubmit` composes a daemon-side turn from the session selection but has no promotion input.

The daemon's authorization decision already carries the authenticated `principal_id`, and `TurnSubmit` is scoped through the session with `AgentInvoke`; request DTOs do not establish identity. Turn admission generates a daemon `turn_id`, acquires the existing durable session-controller lease, and only then calls `run_turn`; the handler returns `Ack` after `run_turn` accepts. That lease has its own SQLite transaction, while there is no durable turn message/admission record coupling it to promotion state. Session schema is additive through migration v70, and the authoritative `CoreDaemon` owns its SQLite pool. This establishes a possible narrow session-owned persistence surface, but work package A still needs to prove how a promotion claim can be recovered without loss or duplicate provider delivery across the gap between runtime acceptance and durable completion state.

### WP-A audit — complete (2026-10-12)

| Boundary | Current owner and evidence | Consequence for C004 |
|---|---|---|
| Request identity and session authorization | `CoreDaemon::handle_request_with_client` derives `RequestAuthorityContext` from the transport-bound `ClientRegistry`; `SessionPromptSubmit` is `AgentInvoke` with `ViaSession` scope in `codegg-core::authorization::operation_descriptor`. Session/project resolution is server-side. | A new stage/query API can derive principal and project from trusted server authority; request DTO identity fields must not be trusted. |
| Local pending promotion | `App::pending_shell_promotions` and `UIMessage` hold staged text in frontend memory. Local `TurnSubmit` success marks it promoted; remote `/shell-include` inserts an unsubmitted composer draft. | Neither route has a canonical durable stage acknowledgement/query or survives frontend restart. |
| Turn request and admission | `TurnSubmit` carries provider messages but no stable submission/idempotency key or promotion IDs. `SessionPromptSubmit` carries one text string and no promotion input, then recurses through `TurnSubmit`. The daemon creates a random `turn_id`; `RequestEnvelope.request_id` is correlation/audit identity, not replay protection. | Duplicate/reconnect requests cannot resolve to one durable accepted turn or prove exactly-once promotion consumption. |
| Acceptance transaction | Turn admission reserves in-memory `active_turn`, separately calls `SessionControllerStore::acquire` (one durable controller-row operation), builds the runtime input, then awaits `run_turn` before returning `Ack`. The controller row contains no prompt or promotion payload. | Existing controller storage cannot atomically record admission plus promotion consumption. The external provider call is outside SQLite transaction scope. |
| Durable session storage | The daemon catalog has sequential SQLite migrations through v70. `session`/`message` store conversation state; `session_turn_controller` stores control ownership; neither models bounded, expiring, principal-bound pending redacted evidence and its consume receipt. | A narrow additive ledger is technically possible, but a migration alone does not answer accepted-turn recovery or ambiguous delivery after daemon restart. |
| Capability compatibility | `SessionPromptSubmit` has no per-feature capability probe; `ChatCapabilities` is for chat and does not advertise human-shell staging. | A new stage lifecycle needs explicit feature negotiation and a typed unsupported-daemon result before the UI reports staged. |
| Restart and provider outcome | Frontend pending state disappears on restart. The server has no durable accepted-turn envelope/provider-delivery receipt; a provider request may have an unknown outcome if the daemon stops after runtime acceptance. | The storage owner cannot currently distinguish an unaccepted pending stage from an accepted turn whose provider delivery outcome is unknown. Choosing consume/replay semantics crosses the current session-storage and turn-runtime boundary. |

**WP-A disposition:** authentication and binding have a viable existing owner, but no existing durable contract can make promotion consumption and accepted turn identity recoverable. `ADR-0017` is proposed to decide the durable lifecycle and restart semantics before WP-B adds a table or protocol. C004 is blocked pending that decision. Do not implement a generic context store or represent composer insertion as staging.

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
