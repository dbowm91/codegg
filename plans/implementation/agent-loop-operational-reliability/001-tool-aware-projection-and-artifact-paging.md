# Agent Loop Operational Reliability M001 — Tool-aware Projection and Artifact Paging

Status: closed

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (main; verify at implementation start)

Source roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`; `plans/000-long-term-specification.md#29-system-invariants`; `plans/001-terminology-and-domain-model.md#7-agent-terms`.

Applicable ADRs: ADR-0040 proposed (M002 decision gate only); ADR-0028 accepted remains binding for shell changes.

Primary class: capability

## 1. Objective

Make `read` and `context_read` deliver the requested contiguous bounded source/artifact slice to the model without an implicit 20-line or recursive compression step. Preserve context budgets, secure artifact handles and existing compaction.

## 2. Why this milestone is ready

Previous context-continuity and tool-surface milestones closed; existing `read`, `context_read`, `ContextArtifactStore`, `ProjectionConfig`, and `AgentLoop` are sufficient. No ADR acceptance or new store is required.

## 3. Current implementation evidence

- `src/tool/read.rs` defaults to 2,000 *lines* and allows offsets; it is not the effective model-visible cap.
- `src/context/projection.rs`: `max_success_tokens=800`; successful outputs above threshold use `truncate_to_lines(output, 20)` and `ctx://` recovery header.
- `src/agent/loop.rs` and `src/agent/follow_up.rs` apply `project_tool_output` to the returned output; test whether `context_read` is being projected again (suspected second truncation).
- `src/context/read_tool.rs`: `max_bytes=20_000` default, byte offset and UTF-8 boundary handling, same-session check.
- `src/tool/broker.rs` has separate display/hard output caps; `src/tool/bash.rs` has 50,000-byte/2,000-line output cap. Do not confuse any of these with projection.
- Existing `[context]` fields allow projection toggles and thresholds; profile-aware limits may further constrain tool results.

## 4. Invariants

No unbounded model-visible results, no loss of authorization/redaction, no altered `ctx://tool` or `ctx://evidence` wire format, exact UTF-8 slice boundaries, stable line numbers and continuation offsets, no stale-content masquerading as complete read, no new artifact/history store. An explicit user-requested offset/page must be honored or rejected with a precise bound.

## 5. Scope

In scope: operation-aware projection and exact bounded `context_read` paging; model-facing output budget selection; explicit complete/truncated/page metadata; integration for initial/follow-up tool paths and tests.

Out: global unlimited tool output, replacing compaction, retuning `bash` process capture, changing permissions, binary/PDF content transport, or adding new model tools.

## 6. Required production changes

**Context:** in `src/context/projection.rs`, separate source-read from generic success summaries. Preserve a contiguous requested line range with appropriate configurable token budget (initial candidate 4–8K tokens for source reads; benchmark against model-profile `max_tool_result_tokens`; no hard-coded unbounded 2,000-line prompt injection). Return exact shown range, total lines/bytes, truncation and deterministic next offset. For `context_read`, deliver the exact authorized bounded artifact page once; prohibit recursive `ctx://` projection or a continuation that points at itself.

**Loop:** share one projection decision between primary and follow-up paths; use a typed projection metadata result if necessary. Avoid setting success=true on incomplete storage/transport. If artifact persistence fails, do not emit a nonfunctional "use context_read" instruction.

**Storage/protocol:** reuse existing store/handles, no migration anticipated; preserve binary/redaction safety and compatibility for existing consumers.

**Security:** apply the same redaction and per-session authorization for passthrough pages; reject byte sizes above a safe configurable maximum and preserve UTF-8 boundaries.

## 7. Ordered work packages

A. Capture failing integration fixtures: `read` of 1,000 lines returns only 20; `context_read` requested page is reprojection-truncated; test success/failure and absent artifact cases. Confirm if the hypothesis reproduces before changing code.

B. Design output budget arbitration: requested `limit`/page, model/profile context budget, broker cap, artifact store and redaction; choose a documented exact continuation. Preserve projections for verbose shell/logs.

C. Implement read/artifact projection and route both loop paths through the same behavior; avoid duplicate counters, fake unlimited modes, or implicit recursive fetch.

D. Add token/byte/line-limit and high-context/low-context tests; update operator docs and a focused regression guard only if a test cannot cover the invariant.

## 8. Failure, cancellation, restart, and contention

A failed artifact write must either provide usable bounded inline output or emit a typed storage failure—not a broken handle. Same-session handles survive restart per canonical store semantics; cross-session fetch is denied. Pagination is read-only/idempotent under concurrent calls; if data revision changes, prevent silently mixing revisions of the same source snapshot. Timeout/cancellation must not claim a complete response.

## 9. Compatibility and migration

No DB schema changes. Keep existing `[context]` keys and `context_read(handle,offset,max_bytes)` compatible; add optional budget knobs only with sane old-config fallback. Existing compressed tool artifacts remain readable.

## 10. Required tests

Focused: `context::projection`, `context::read_tool`, `tool::read`; 0/1/20/21/2,000 line fixtures, long lines, multibyte UTF-8, byte caps, exact next offset. Integration: scripted provider requests 1,000 lines then `context_read` and observes contiguous complete slices; no second 20-line collapse in ordinary or follow-up path. Restart: stored artifact after store re-open. Contention/cancel: two readers, cancelled paging. Security negatives: wrong session, redaction, malicious/oversized `max_bytes`, nonexistent handle. Compatibility: old config and `ctx://` formats.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Run applicable focused tests from §10 *before* broad verification; use `cargo test -p codegg --lib <module_filter> --locked` and/or the named existing integration target, adjusting command to the actual crate/target at implementation time. Record exact commands and results; never claim verification that was not run. No new CI lane, permanent benchmark, network-dependent provider tests or SBC compile/startup test.

## 12. Documentation updates

Update the relevant `architecture/{agent,tool,context-ledger,provider,permission}.md` sections only where behavior changes; update `docs/tools.md` and `AGENTS.md` if operator/model-visible advice changes. Preserve completed predecessor closure records.

## 13. Acceptance criteria

1. A source read spanning >800 estimated tokens no longer returns only the first 20 lines by default; the visible portion is a complete contiguous bounded range, with usable continuation.
2. A `context_read` result is never recursively projected into another inaccessible/misleading preview.
3. Current security, budget, compaction, and handle regression tests pass; oversized output is bounded without throwing away requested detail.
4. Integration test proves reduction in forced retrieval calls for a real source-read trajectory, without claiming live-provider performance.

## 14. Stop conditions

Stop and report rather than broaden scope if the implementation demands a new authorization owner, durable-store migration without a reviewed design, an incompatible provider contract, a cross-boundary bypass, or a new ADR. If source differs materially from pinned baseline, record the exact discrepancy and update the smallest possible plan section before implementation. Do not mark closed until a `plans/closure/agent-loop-operational-reliability/NNN-status.md` record contains commits, failing-then-passing evidence, requirement-to-test matrix, cancellation/contention/security results, residual risks and the exact verified head.

## 15. Handoff

Execute only this milestone's scope. Preserve unrelated working changes, pin the initial commit and provider/model fixture, run the required tests, and register the closure or a named blocker in `plans/registry.md` using the planning skill.
