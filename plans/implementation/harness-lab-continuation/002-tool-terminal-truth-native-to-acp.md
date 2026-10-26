# Harness Lab Continuation D002 — Truthful Native Tool Terminal Events Through ACP

Status: ready
Baseline: `main@957fcbf00e17bf108429f0d75dcbf760736ae511` (2026-10-26; refresh on entry).
Roadmap: `plans/subsystems/harness-lab-continuation-corrective-addendum.md`.
Findings: `plans/closure/harness-lab-post-pilot-corrective/005-status.md#10-unresolved-findings`; C006's typed gap events. Normative: `plans/000-long-term-specification.md` §§23–24, 26–29, `architecture/projection.md`, `architecture/tool_broker.md`. Primary class: invariant.

## 1. Objective

A **real failed tool invocation** must be published as failure from the native tool broker/turn to projection, ACP, and Harness Lab; not as a completed success containing error text. Fix the **producer truth** rather than relying on Python to parse tool output. Preserve replay/disclosure/ordering and compatibility with ACP clients.

## 2. Evidence and read-across

The C005 closure reports a native `CoreEvent::ToolCompleted { success: true }` for an errored call and no `ProjectionEvent::ToolFailed` producer. The harness currently infers errors from an output envelope. Re-check exact event production paths under `src/core`, `src/agent`, `crates/codegg-core/src/projection_replay`, `crates/codegg-protocol/src/projection`, and `src/acp.rs`. In addition, C006 now emits `codegg_event_gap` for dropped projection events; **unknown due to a gap** is not a known tool failure. Do not conflate transport loss, native tool failure, structured denial, cancellation, error-shaped legitimate output, provider failure, and process exit codes.

## 3. Invariants

- One tool identity has one terminal **success/failure/denied/cancelled** interpretation, with correctly correlated session/turn/tool call IDs.
- Success means the trusted native executor reported success; a text string resembling an error does not override its structured success. Denial is never a successful call. Internal errors aren't silently converted into tool results labeled `completed`.
- Public projection cannot expose secret tool arguments/raw stderr/provider credentials; uses existing `VisibilityClass` and authorized snapshot filtering.
- ACP v1 clients keep standard notification/stop shapes; any additional fields are optional, namespaced, bounded, and correctly ordered. No hidden prompt/response body in errors.
- A gapped/incomplete stream is **unmeasured**, never automatically a failed tool or candidate regression. Replay duplicates cannot produce two different terminals.
- Source of truth remains native daemon/tool broker; Python only validates/adapts.

## 4. Dependencies and classification

Ready independently of D001/D003. Interface coordination with D003 for ACP update names and loss accounting; do not add a second event bus, daemon scheduler or Python tool-execution layer. A protocol-breaking change needs a dedicated ADR/migration decision; avoid it.

## 5. Ordered work packages

A. **Failing-first native reproduction:** via local deterministic provider fixture, run a tool that returns typed `ToolError`, a permission Deny, a syntactically error-like *successful* string, a failed Bash subprocess and a cancellation. Capture authoritative executor, `CoreEvent`, projection reducer and ACP notification pair for the **same tool_call_id**. Demonstrate the C005 false completed receipt before fix.

B. **Canonical terminal outcome producer:** identify one layer converting real tool result into `CoreEvent::ToolCompleted.success` and/or `ToolFailed` projection. Update it to derive status from actual structured native result (not serialization of text), preserve typed denial vs ordinary failure where native taxonomy supports it, and ensure durable replay transforms exactly the same outcome. Do not modify independent evaluator/verifier to make it pass.

C. **Projection and ACP parity:** make `ProjectionEvent::ToolFailed` (or the existing equivalent authoritative failure shape) originate for genuine failure; update reducer snapshots and safe publication; verify `src/acp.rs` reports `tool_call_update.status=failed` and stable tool id. Confirm no fake `session/prompt` failure: the model may recover and successfully finish the turn after a tool fails. During event-stream gap retain `codeggEventGap` and mark outcome observationally incomplete as C006 requires.

D. **Python ledger semantics:** remove brittle string parsing as a primary truth source; use typed ACP statuses and explicit `failure_class` while reading old immutable receipts through a **legacy-labeled** compatibility path. Match tool starts to one terminal by ID; report unknown terminal on gap and count `recovery_followups` only when a verified tool failure preceded a new call.

E. **Regression matrix:** prove old/new client compatibility and re-subscribed projection replay, multiple parallel tool calls, cancellation race, idempotent delivery, failure followed by successful retry, and receipt size/security bounds. Check changing the outcome does not enable unauthorized operation or override default sandbox.

## 6. Failure/restart/contention

Interrupted tool, daemon restart, projection lag, duplicate delivery and two concurrent subjects each have distinct provenance. If durability does not know whether a tool finished, report `unknown` with verified source, not a guessed success/failure. Do not replay a potentially mutating action merely to obtain a missing notification.

## 7. Compatibility and migration

Prefer no wire schema change: the enum variant `ProjectionEvent::ToolFailed` already exists. Additive ACP fields permitted; legacy `ToolCompleted` records remain interpreted by original stored status and flagged historical rather than rewritten. No production DB migration unless required; document any replay/event-log compatibility.

## 8. Verification

```bash
cargo test -p codegg --lib acp
cargo test -p codegg --test acp_stdio
cargo test -p codegg --lib projection
python3 -m unittest discover -s dev/harness_lab/tests -v
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```
Run focused module tests in their owning crate if filtered `codegg` lib tests are zero. Add failing-first assertions against the prior producer, emitted wire result and replay. No paid model calls are necessary or authorized by this plan.

## 9. Acceptance / stop / closure

Accept only with native error -> failure terminal -> authorized projection -> ACP status -> Python metric proof; a successful error-looking body stays success; explicit denial stays denied; incomplete gap stays unmeasured; retry recovery remains possible; test suite and safety guards pass. Stop for ABI/protocol authority change, secret disclosure or mutation replay.

Closure `plans/closure/harness-lab-continuation/002-status.md` should include source-to-wire event table, exact IDs, before/after negative tests, compatibility, CI SHA, and audit for whether C005's high-severity finding is actually discharged. Do not edit historical closure records to erase prior discrepancy.
