# Delegated Live Transcript Milestone M001 — Closure Status

- **Plan**: `plans/implementation/delegated-run-live-transcript/001-live-semantic-capture.md`
- **Roadmap**: `plans/subsystems/delegated-run-live-transcript-roadmap.md` §7 (M001)
- **Branch**: `plan/delegated-run-live-transcript`
- **Status**: closed

## 1. Executive finding

The delegated run's transcript was previously written in a **batch after the
child finished**: the worker walked the finished event vector and the loop's
retained outcomes and appended the rows once the run was over. A parent that
opened the child's transcript while the child was still running saw nothing at
all, and a child that died mid-turn left a transcript that described only its
prompt.

The capture now happens **inside the run**. The agent loop reports the
semantic checkpoints it reaches — the model's assistant message for a turn, a
tool call it has accepted, a tool result it has delivered to the model — and a
store-backed sink appends each one to the durable transcript at the seam where
it happened. A live child that is blocked inside a tool has already committed
the call that put it there.

The batch reconstruction is gone. There is one writer, one door
(`AgentRunTranscriptStore::append`), and the same write-time redaction and
bounds that applied to the batch.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A live child blocked before its tool returns has already committed the earlier call, without completion of its parent run | `tests/agent_loop/delegated_transcript.inc` — `a_live_child_has_committed_its_tool_call_before_the_tool_returns` | pass | The call row is committed at the acceptance seam, before the tool runs; the result row does not exist while the tool is blocked; the run is still in flight |
| That tool's actual result and the child's following output become durable without the post-run loop as the only recording source | `src/agent/loop.rs` (three capture seams), `src/agent/transcript_capture.rs` (the only writer), `scripts/check_delegated_transcript_store.py` (the batch markers must be absent) | pass | The post-run reconstruction is deleted; the guard rejects its return |
| Cancellation/provider failure retains the committed prefix, does not invent a successful result, and states truncation/failure honestly | `a_failed_child_leaves_the_observations_it_made_before_the_failure`; `src/agent/worker.rs` (the failure path no longer depends on `events?`; the classified terminal note) | pass | The committed prefix survives; the note reads `Subagent task failed: … [class=provider_authentication]` |
| All existing C002/C004 redaction, cap, continuation, and pending-call evidence still passes; no synthetic user session or new execution owner appears | `cargo test --test agent_loop -- delegated_transcript` — 10 passed, 0 failed (6 pre-existing, unchanged) | pass | No new session row, no new execution owner |
| A known-secret test inspects stored rows, and a regression test fails on the old post-run-only tree | `a_delegated_run_redacts_a_secret_in_the_tool_output` (pre-existing, raw stored row inspected); `scripts/check_delegated_transcript_store.py --self-test` injects the post-run reconstruction and the removal of each seam and proves the guard rejects all 10 injected defects | pass | The guard is not vacuous |
| The model's assistant message is recorded at a completion/tool boundary, not one write per delta, and never the provider's private reasoning | `src/agent/loop.rs` (`capture_assistant_message(processor.text())` at the provider-response boundary); `a_child_records_no_reasoning_row` | pass | One row per provider response; the reasoning text appears nowhere in the transcript |
| A call is recorded when the loop accepts it, and its result when the loop delivers it to the model | `src/agent/loop.rs` (`capture_accepted_tool_calls` before tool execution; `capture_tool_result` after `request.messages.push(msg)`); `the_transcript_records_a_call_and_its_result_in_ordinal_order_with_one_row_per_call` | pass | Ordinal order: call < result; exactly one row each |
| One logical tool call and its result appear at most once per recorded attempt | `src/agent/transcript_capture.rs` (`recorded_calls` and `recorded_results` claim sets) | pass | A repeated call id produces neither a second call row nor a second result row |
| Writer failure is classified and observable, never reported as an empty transcript | `src/agent/transcript_capture.rs` (`write_failed` / `recording_stopped`), `src/agent/worker.rs` (post-run health log) | pass | The sink appends an honesty note on a failed write |

## 3. Production implementation evidence

### New module — `src/agent/transcript_capture.rs`

| Item | Role |
|---|---|
| `TranscriptCheckpoint` | The three semantic checkpoints the loop reports: `AssistantMessage`, `ToolCall`, `ToolResult` |
| `AgentRunTranscriptCapture` | The loop's contract: `async fn record(&self, checkpoint)`; it never fails the loop |
| `StoreTranscriptCapture` | The only writer. Holds the store, the run id, the per-attempt `recorded_calls` claim set, `recording_stopped` and `write_failed` |

The sink writes every body through `AgentRunTranscriptStore::append`, so the
store's write-time redaction, its per-row byte bound and its per-run byte
bound apply to a live capture exactly as they applied to the batch. The row
bodies keep the shapes a reader already parses:

- `tool_call_id={id} tool={name} arguments={args}`
- `tool_call_id={id} tool={name} success={bool} output_truncated={bool} output={text}`

### The loop — `src/agent/loop.rs`, `src/agent/coordinator.rs`

`AgentLoopServices` gained `transcript_capture: Option<Arc<dyn
AgentRunTranscriptCapture>>`. It is `None` for every turn that is not a
durable delegated run, so an ordinary turn records nothing and behaves
identically. `AgentLoop::set_transcript_capture` installs it.

Three seams, each a semantic boundary rather than a stream event:

1. **The provider response is complete** → the model's assistant message for
   that turn. `processor.text()` is the model-visible assistant content:
   complete, never a partial delta, and never the provider's private
   reasoning (which the processor holds separately).
2. **The loop has accepted the structured calls** → one `ToolCall` row per
   logical call, **before** any tool runs. A reader sees a call that is still
   pending. This also covers the textual tool-call repair fallback, whose
   calls the old batch could not see at all.
3. **The loop has delivered the result to the model** → one `ToolResult` row
   carrying the redacted, bounded text the model received and the tool's
   actual outcome.

### The worker — `src/agent/worker.rs`

The sink is built from the run's own transcript store and installed on the
child's loop before the run starts. The post-run batch reconstruction is
deleted. The failure path no longer depends on `events?`: a failed run keeps
the observations it had already made, and the classified terminal note is
written by the caller.

The failure note now names its class (`classify_delegated_failure`), so a
note whose failure has no recognised class says `[class=unclassified]` rather
than implying a class it does not have.

### The guard — `scripts/check_delegated_transcript_store.py`

A new rule, `delegated-transcript-capture-not-in-execution`, requires the
loop to call each of the three capture seams and the capture path to install
the sink, and rejects the markers of a post-run batch reconstruction. The
rule's `--self-test` injects each of those defects and proves the guard
rejects them: 10 injected defects, 10 rejections, and no finding on the
unmodified tree.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo check --lib
cargo test --test agent_loop -- delegated_transcript
python3 scripts/check_delegated_transcript_store.py
python3 scripts/check_delegated_transcript_store.py --self-test
bash scripts/check-core-boundary.sh
```

### Results

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo check --lib` | no errors, no warnings |
| `cargo test --test agent_loop -- delegated_transcript` | **10 passed, 0 failed** — 6 pre-existing (unchanged) and 4 added |
| `scripts/check_delegated_transcript_store.py` | ok |
| `scripts/check_delegated_transcript_store.py --self-test` | 10 injected defects rejected; the unmodified tree produced no finding |
| `scripts/check-core-boundary.sh` | passed |
| `scripts/check_test_assertion_quality.py` | all checks pass |
| `scripts/check_test_duplication.py` | all checks pass |
| Every guard in `scripts/verify.sh quick`'s `run_guards()` (82 guards) | 0 failures |

### The full workspace sweep, and why its failures are not this line's

`XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci --no-fail-fast`
runs **16,600** tests across 59 binaries: **16,569 passed**, **30 failed**,
**1 timed out**, 9 skipped, 3 leaky.

Every one of those 31 non-passing tests was checked against the **baseline**
(this branch's `HEAD`, with this line's changes stashed). **29 of the 30
failures reproduce identically on the baseline** — they are pre-existing
failures of this tree, not of this line: two `daemon_refresh` tests that
reject `/tmp` as a workspace root on macOS, the `tool_programs` family, the
`contained_worktree` grants, the git capture-cache read, the scratch-cache
root contract (which the private `XDG_CACHE_HOME` above deliberately
contradicts), the eggsearch summary projection, the managed-process sandbox
refusal and the bash workdir guard. The thirtieth — the git capture-cache
repeat read — **passes in this tree when it is run on its own**, with and
without the private cache root, so its sweep failure is a concurrency flake.
The timeout — `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
— passes in isolation in 0.13 s and is likewise a contention artefact of the
sweep's parallelism.

The line's own surface is green: the `agent_loop` family is **563 passed,
2 failed**, and both failures are the two pre-existing ones above
(`agent_loop_harness::test_live_dispatcher_uses_execute_capture` and
`agent_loop_harness::task_tool_status_get_group_and_cross_session_contracts_are_model_visible`),
which reproduce identically on the baseline. The `delegated_transcript`
member itself is **10 passed, 0 failed**.

`cargo clippy --workspace --all-targets --locked -- -D warnings` reports
**13 errors, none of them in a file this line changed**: they are in
`src/search_backend/framing.rs`, `src/search_backend/eggsearch.rs`,
`src/eggsact/adapter.rs`, `src/test_runner/{runner,index}.rs` and
`src/tool/{task,skill,edit,deterministic}.rs` — files this line does not
touch, and lints this tree already carried. Every file this line changed is
clippy-clean.

### The blocking mechanism, and why the live test is not vacuous

The live test's child reads a **FIFO** inside its own workspace, and the
writer is a dedicated OS thread: `open(O_WRONLY)` on a FIFO blocks until a
reader opens it, so the writer's `open` returning is proof that the child's
`read` has already opened the pipe and is blocked inside `read_to_string`.
The pipe holds neither data nor EOF until the test releases it, so the tool
**cannot** have returned. No sleep and no elapsed-time inference is used
anywhere.

The test was proved non-vacuous by a mutation run: the poll predicate was
flipped from the `ToolCall` row to the `ToolResult` row, and the test then
failed on its bounded 10-second poll, dumping the live transcript at that
instant — ordinal 0 `Internal/ModelMessage`, ordinal 1
`Orchestrator/ModelMessage` ("read the pipe"), ordinal 2 `ToolCall`
(`tool_call_id=call-live tool=read arguments={"path":"pipe-672a8c3b…"}`) and
**no** `ToolResult` row. That row set is the capture happening *during* the
run, not a post-run batch. The mutation was reverted afterwards.

The live test is `#[cfg(unix)]` and was exercised on macOS. It was not
exercised on Linux or Windows.

## 5. Invariant review

| Invariant | How it holds |
|---|---|
| The store is the only door | Every body reaches it through `AgentRunTranscriptStore::append`. No second journal, no bypass. |
| The loop owns no durable state | The loop holds a trait object; the sink owns the claim set and the health flags. |
| No reasoning is stored | The seam records `processor.text()`, never the reasoning content part. |
| A call is not a result | The two are separate checkpoints with separate kinds, and the result is only written after the model received it. |
| No execution authority changed | The capture observes; it never dispatches a tool, never bypasses the broker, and never runs outside the loop's own seams. |
| No new execution owner | The sink is created by the worker that already owns the run, and it holds no process or task of its own. |
| No unbounded queue | There is no queue: the sink appends synchronously at each seam, and the store's per-run byte bound truncates. |

## 6. Failure and recovery review

| Scenario | Behaviour |
|---|---|
| The child is cancelled mid-tool | The committed prefix survives. The cancelled run's terminal note is written by the cancellation path. |
| The provider fails mid-run | The committed prefix survives. The classified terminal note is written by the failure path. |
| The store write fails | The write is logged, `write_failed` is set, and a best-effort `TerminalNote` says the transcript is incomplete. The delegated work is unaffected. |
| The store's per-run bound is reached | The store refuses further appends with `RunTruncated`; the sink sets `recording_stopped`, logs it, and the post-run health log records it. The truncation is visible in the store's own read. |
| The child is restarted as a continuation | The continuation seeds from the rows the previous attempt committed. The claim set is per attempt, so one logical call is one row per attempt. |

## 7. Migration and compatibility review

No schema migration. The store's tables and bounds are unchanged; only the
*timing* of the writes changed. A transcript written by the old build is
readable by the new one and vice versa: the row kinds, the roles and the body
shapes are identical. The one visible difference is that a multi-turn run now
records one assistant message row per provider response rather than one
concatenated row for the whole run — which is what the recorded conversation
actually was, and what the continuation seeding now replays.

## 8. Security review

The sink writes only through the store, so the store's write-time redaction
applies to every body, including the assistant message. The known-secret test
inspects the stored row and still passes. The capture never stores the
provider's private reasoning. No credential, path or tool argument is copied
outside the store's own redaction and bounds.

The transcript is written by the child that owns the run, into the run's own
store. No session row is created and no user-visible surface is added by this
milestone.

## 9. Documentation and operations

| Document | Change |
|---|---|
| `architecture/agent.md` | New section "Delegated-run transcript capture (in execution)"; the module table gained `src/agent/transcript_capture.rs` |
| `architecture/codegg_core.md` | The delegated-transcript section now describes the in-execution capture instead of a post-run batch |

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The assistant message row is the model-visible text; the loop's path redaction (`redact_local_paths`) applies to tool output but not to the assistant message | A path the model mentioned in prose stays in the transcript | Open. M002's read surface serves the store's already-redacted bodies, and the store's redaction is credential redaction, not path redaction; a later line may path-redact the assistant row at capture |
| low | A repeated call id inside one provider response keeps the first call's arguments | The batch kept the last | One logical call is one row either way; the provider's behaviour is pathological. The result side is claimed separately, so a repeated id produces neither a second call row nor a second result row |

No finding blocks the milestone.

## 11. Roadmap disposition

M001 is closed. M002 (the authorized cursor read and the commit-derived
publication) and M003 (the TUI live follow) remain open and depend on M001's
capture contract, which is fixed and documented in
`architecture/agent.md`.

## 12. Registry updates

`plans/registry.md` records M001 as closed, and the subsystem roadmap as
closed with M002 and M003.
