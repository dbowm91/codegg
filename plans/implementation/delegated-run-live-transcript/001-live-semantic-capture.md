# Delegated Run Live Transcript M001 — Capture Semantic Conversation While Running

Status: ready for handoff
Primary class: infrastructure
Repository review baseline: main 0be3e0a53ba97ee51f6d64aba2235bdef92452e7, 2026-10-26. Re-read implementation HEAD before changing code.
Source roadmap: plans/subsystems/delegated-run-live-transcript-roadmap.md §7 M001.
Long-term: plans/000-long-term-specification.md (daemon authority, agent hierarchy, reliability, observability); plans/001-terminology-and-domain-model.md (delegated run versus session).
Accepted ADRs: ADR-0051 (durable, redacted, bounded delegated transcript); ADR-0043 (session-owned child lifecycle); ADR-0041 (parent-mediated human steering). Predecessor closure: plans/closure/delegated-run-observability-and-recovery-corrective/002-status.md and /003-status.md. Do not reopen those historical milestones.

## 1. Objective

Move the durable child's conversation capture from the post-return batch in worker.rs into well-defined *in-execution semantic checkpoints*. During a deliberately stalled tool execution, the store must already contain preceding model messages, initiated tool calls and completed tool results, so a reader does not need to await the entire agent_loop.run() call. Capture must also survive an error or cancellation without claiming an unexecuted tool completed.

## 2. Why ready

C002 already supplies a store with append, ordered ordinals, bounded read, write-time redaction and per-run truncation. Worker.rs already captures the compiled prompt before provider entry. AgentLoop already retains executed tool outcomes and provider events. No new identity/store contract or ADR is required; this milestone changes only the capture *when* and its deterministic edge cases. It deliberately does not introduce frontend/publication controls owned by M002/M003.

## 3. Baseline evidence and unclosed requirement

- src/agent/worker.rs calls capture_transcript twice before its child run (compiled prompt, requested prompt). After agent_loop.run(request).await and successful events?, it iterates ChatEvent::ToolCall and executed_tool_outcomes, pairs them by tool_call_id and appends tool calls/results, then appends one final ModelMessage. This can produce no intermediate tool or model rows while the child is running; if run() returns Err, the post-run pass is skipped.
- src/agent/loop.rs::retain_executed_tool_outcome is where executed result provenance, success and already-bounded model-visible output are available; raw provider event output does not supply a ToolResult. Its retained-outcome cap and truncation provenance are already documented and tested.
- crates/codegg-core/src/agent_run_transcript.rs::AgentRunTranscriptStore::append applies redaction/size bounds. MAX_TRANSCRIPT_ROW_BYTES=8 KiB; MAX_TRANSCRIPT_RUN_BYTES=256 KiB. Sqlite and in-memory implementations share the preparation rule.
- C002 closure tests include a pending call with no result, tool-result provenance, output truncation, and a known secret absent from raw stored SQL. These are hard regression gates.
- No current standalone/legacy delegated path is promised a durable run id. M001 cannot manufacture one.

## 4. Invariants

- Store only semantic content the child was actually given or produced; no fabricated ToolResult, no unexecuted tool action, no raw provider reasoning, no unredacted or unbounded result.
- Existing AgentRunId, attempt authority, output/report extraction, tool event vector and scheduler status/ownership are unchanged. Do not add child session rows.
- One logical tool call and its matching executed result appear at most once per recorded attempt; preserve call/result ordering and tool_call_id association, including duplicate provider ids and the lack of an outcome. An interrupted or failed child never receives a synthetic successful result.
- Existing redaction and truncation are enforced *before durability* by the one shared store path; neither the loop nor a later reader may bypass it.
- A storage failure must be classified and observable, not misrepresented as 'empty transcript'. Do not silently convert it to fabricated success or alter provider/agent execution control flow without a separate decision.

## 5. Scope

In scope: an optional durable-child capture observer in the agent loop; semantic checkpoints for provider assistant messages and tool calls; executed tool outcomes captured near completion; terminal/error/cancelled attempt note; deduplication/ordinal correctness; light capture diagnostics; tests and relevant architecture updates.

Out of scope: a new DB table, realtime TUI, CoreRequest, CoreEvent, projection DTO or subscription; tokens-by-token persistence, exposing hidden reasoning, capturing unrelated parent user sessions, retry/continuation policy changes, new permissions or background execution.

## 6. Required production changes

Core/domain: reuse AgentRunTranscriptStore and its existing row kinds/roles. Add a small optional capture contract to the loop or its supervisor, injected only for a durable delegated run. It must receive finished semantic content and executed tool outcomes; the loop must not import a TUI dependency, publish projection events or invent a second journal.

Storage/migration: none expected. If the existing append/store transaction semantics cannot guarantee monotonic ordinals across an attempt, document the defect and stop before schema change.

Protocol/DTO: no change in this milestone.

Runtime/concurrency:
- Emit semantic assistant message text at a completion/tool boundary, not one SQL write per TextDelta; hold a bounded pending buffer with explicit flush on error/cancellation/normal completion, including UTF-8-safe truncation at shared store bounds. Captured text must not differ from the model-visible/generated semantic result through optimistic reconstruction.
- Record a ToolCall once when the loop has accepted the provider's structured call; associate it with the actual completed ToolResult at the point the result is known and the loop has delivered it to the model. Failed tool execution records actual failure, not success. A pending call has no result row.
- Arrange append order such that a live cursor read sees calls before their results. Multiple sibling children can append concurrently but ordinals remain per run; a continuation uses the same run identity and never resets the global ordinal.
- Keep the existing post-run report/output extraction for model/tool execution semantics. Remove/guard only redundant post-return *recording* once in-loop capture is proven. A run returning Err still flushes finished observations and an appropriately classified terminal note without relying on events?.
- Bound any internal queue and record explicit overflow/truncation; no unbounded per-delta tokio::spawn, detached writer, or worker-task lifetime leak. If append is async, avoid awaiting it under tool broker or scheduler locks.

Frontend: none.

Security: any new pathway must call the same store append. Existing per-kind redactor and environment filter are not loosened; no tracing/debug logs of raw bodies.

Docs/guards: update architecture/agent.md and architecture/codegg_core.md for capture timing (not a UI live streaming claim). Strengthen scripts/check_delegated_transcript_store.py with a tested non-vacuous rule against reverting to post-run-only capture, or document why an executable integration assertion provides stronger non-vacuity. Update relevant test references, avoid editing closed closure claims.

## 7. Ordered work packages

A. Locate the exact loop semantic boundaries. Prove which events are generated, which tool outcomes are executed, and which failure paths leave the run() result unavailable. Write a failing test in a durable-child fixture that blocks a tool after a call and reads the store before agent_loop.run() returns. Record current baseline failure.

B. Implement optional, bounded in-loop capture through the current store contract. Preserve provider-visible response/event path and existing execution budget. Pair accepted call/result by tool_call_id, avoid output fabrication. Track append outcomes and recording-stopped state.

C. Make terminal capture reliable on success, error, cancellation and continuation. Ensure flush is not skipped by early returns and that stale/failing child stops cannot incorrectly mark success. Remove the existing post-run duplication.

D. Add regression/security/performance tests, update source docs and guards. Re-run existing C002 and C004 tests for continuation seeding and known-secret redaction.

## 8. Failure, cancellation, restart and contention

A tool mid-flight: committed preceding entries are immediately queryable; pending call has no result. Child cancel/error: committed entries remain, terminal note reflects actual outcome; any incomplete buffer is flushed or explicitly marked truncated. Daemon killed after append: rows survive exactly once; daemon killed before append: content is not claimed as stored. Concurrent children: isolate by durable run id. Continuation: appended records remain monotonic across attempts and the original continuation seed still derives from exactly the authorized durable history. Writer failure: log bounded diagnostics and surface recording availability through existing store read/status; do not let an unchecked send failure silently create a false complete transcript. Preserve any existing fail-closed provider-entry semantics in ADR-0051.

## 9. Compatibility and migration

No schema or protocol migration. Existing stored transcripts remain readable. Legacy non-durable children keep explicit absent/unavailable semantics. Old clients receive the same snapshot as C003. Do not promise in-flight observer data in a frontend until M002/M003.

## 10. Required tests

Focused: semantic assistant chunk flush/UTF-8 boundary; executed call/result pairing; tool denial/failure; pending call; no duplicate post-return entry; capture buffer cap; overflow/recording-stopped state; no ReasoningDelta row.

Integration: real SQLite durable delegated run blocked at a tool barrier; assert prompt + accepted call already persisted before the tool returns, then unblock and assert result and final message in ordinal order. Error and cancellation at tool barrier must leave correct partial transcript and no invented result; continue same run and verify ordinals do not reset.

Negative/privacy: store raw SQL read shows known credential-shaped values absent in all message, arguments and output classes; the live observer never bypasses the redactor. Concurrency: two children interleaved without cross-run row mixing. Restart: kill/reopen SQLite pool between checkpoints, prove committed prefix survives. Legacy path: no store fabricated.

## 11. Verification commands (to run, not claims of execution)

- cargo test -p codegg --locked --test agent_loop -- delegated_transcript
- cargo test -p codegg --locked --lib -- agent::worker
- cargo test -p codegg-core --locked --lib -- agent_run_transcript
- cargo test -p codegg --locked --test agent_loop -- delegated_continuation
- python3 scripts/check_delegated_transcript_store.py --self-test
- bash scripts/check_projection_disclosure.sh --self-test
- python3 scripts/check_execution_ownership.py
- python3 scripts/check_scheduler_bypass.py
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -- -D warnings
- bash scripts/verify.sh quick

Record native platform constraints, failed/unrun commands and external CI results honestly.

## 12. Documentation

architecture/agent.md; architecture/codegg_core.md; store/capture comments; relevant harness skill guidance. Only update docs/tui.md after M003 closes.

## 13. Acceptance criteria

1. A live child blocked before its tool returns has already committed the earlier call, without completion of its parent run.
2. That tool's actual result and the child's following output become durable without the post-run loop as the only recording source.
3. Cancellation/provider failure retains the committed prefix, does not invent a successful result, and states truncation/failure honestly.
4. All existing C002/C004 redaction, cap, continuation, and pending-call evidence still passes; no synthetic user session or new execution owner appears.
5. A known-secret test inspects stored rows, and a regression test fails on the old post-run-only tree.

## 14. Stop conditions

Stop for an unresolved choice that changes execution authority, creates a session row, requires storing raw reasoning, changes provider-visible event ordering, requires an unbounded capture queue, or cannot distinguish executed from merely proposed tool actions. Escalate with a focused ADR or corrective split rather than manufacturing transcript content.

## 15. Closure evidence

Provide commit(s)/PR, focused mid-run assertion with synchronization barrier and baseline-failing proof, failure/cancel/restart/continuation matrix, byte/row/truncation evidence, known-secret raw-SQL test, guard self-tests, verification command outcomes, exact-head status, known limitations and registry/roadmap status update.

## 16. Handoff

One agent owns this milestone. Do not concurrently edit worker.rs/loop.rs with a separate M002 producer until interfaces are agreed. Preserve unrelated in-flight work and freeze tests that exercise asynchronous ordering; no live model/provider is required for the primary qualification.
