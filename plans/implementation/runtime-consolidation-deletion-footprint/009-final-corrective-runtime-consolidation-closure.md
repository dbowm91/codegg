# Runtime Consolidation, Deletion, and Footprint M009 — Final Corrective Compatibility, Ownership, Measurement, and Closure Pass

Status: closed — see `plans/closure/runtime-consolidation-deletion-footprint/009-status.md`

Source roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Planning governance:

- `plans/003-planning-process.md`, especially section 7 (Corrective passes)

Original milestones and closure records corrected by this pass:

- M001: `plans/implementation/runtime-consolidation-deletion-footprint/001-legacy-background-scheduler-deletion.md`
- M001 closure: `plans/closure/runtime-consolidation-deletion-footprint/001-status.md`
- M003: `plans/implementation/runtime-consolidation-deletion-footprint/003-agent-loop-ownership-decomposition.md`
- M003 corrective extraction: `plans/implementation/runtime-consolidation-deletion-footprint/008-m003-corrective-physical-extraction.md`
- M003 closure: `plans/closure/runtime-consolidation-deletion-footprint/003-status.md`
- M006: `plans/implementation/runtime-consolidation-deletion-footprint/006-measured-dependency-binary-cleanup.md`
- M006 closure: `plans/closure/runtime-consolidation-deletion-footprint/006-status.md`
- M007: `plans/implementation/runtime-consolidation-deletion-footprint/007-integration-verification-closure.md`
- provisional M007 closure evidence: `plans/closure/runtime-consolidation-deletion-footprint/007-status.md`

Relevant long-term and architecture references:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`
- `architecture/agent.md`
- `architecture/jobs.md`
- `architecture/scheduler.md`
- `architecture/testing.md`
- `architecture/tool.md`

Repository baseline reviewed: `f1e4c16f1bfe16cad57fb6fc290d48ab03974072`

Primary class: correctness / compatibility / corrective integration

Dependencies:

- hard: M001–M005 production changes are already landed and must not be reopened except for the concrete regressions named below;
- interface: the existing durable `Schedule*` protocol/store/service path and the existing provider event/adapter contract;
- operational: one ordinary existing hosted `CI / verify` run on the exact accepted final candidate after all production corrections and documentation reconciliation;
- sequencing: final M006 measurements and strict M007 closure are controlled by this corrective pass and MUST occur only after the production corrections in work packages B and C are complete.

Target corrective closure record:

- `plans/closure/runtime-consolidation-deletion-footprint/009-status.md`

M006 and M007 retain their own requirement sources and closure records. M009 must update those records truthfully after the final-tree evidence is obtained; it must not erase the fact that the earlier M006 record was blocked or that the earlier M007 record was only provisional/conditional evidence.

## 1. Objective

Close the small but material gaps discovered by the post-implementation audit without creating another architecture layer or roadmap.

The corrective target is:

1. restore the existing TUI background-task capability by routing its live `TaskSchedule`/`TaskList`/`TaskDelete` user flows to the already-authoritative durable schedule API, without restoring `BackgroundScheduler`;
2. finish the remaining physical provider-turn ownership extraction so `provider_turn.rs` is not a façade that immediately delegates streaming/retry implementation back into `AgentLoop`;
3. rerun M006 dependency/feature/release measurements on the actual final consolidated tree and close M006 only if its original acceptance criteria are now satisfied;
4. reconcile stale M003/M008/M006/M007 planning status and commit evidence;
5. run the one existing broad/hosted closure pass required by M007 and close the roadmap only if the exact final candidate is green and all required measurements are complete.

This is a corrective closure pass, not authorization for additional refactoring.

## 2. Post-implementation audit findings

### Finding A — active TUI task capability regressed after M001

Severity: medium.

Current repository evidence:

- M001 correctly deleted `src/agent/task.rs`, the independent timer loop, alternate task persistence interpretation, and the UUID-string-to-`u64` dispatch bridge.
- M001 changed retained legacy `TaskSchedule`, `TaskList`, and `TaskDelete` daemon requests to explicit unsupported responses.
- `src/tui/commands/tasks.rs` still actively constructs those three legacy requests.
- `src/tui/runtime/command_dispatch.rs` still routes the live `TuiCommand::TaskSchedule`, `ListTasks`, and `DeleteTask` paths to those handlers.
- the M001 closure record explicitly notes that the old task UI is non-functional.

This violates M001's compatibility requirement: a live supported caller was discovered, so it should have been mapped to the durable schedule contract rather than left pointing at a deliberately rejected compatibility request.

The corrective direction is not to restore the legacy scheduler. The TUI must consume the durable schedule API.

### Finding B — provider-turn extraction remains partly façade-only after M003

Severity: medium/low.

Current repository evidence:

- the M003 corrective pass materially reduced `src/agent/loop.rs` and physically moved context policy and tool-batch execution to `context_runtime.rs` and `tool_batch.rs`;
- `src/agent/provider_turn.rs` currently contains an adapter whose `receive()` method delegates directly to `AgentLoop::stream_with_retry_impl()`;
- provider streaming/retry/normalization implementation therefore still has a substantial body owned by `loop.rs`, contrary to the intended provider-turn ownership boundary.

The corrective scope is only the remaining physical ownership move. It must not redesign provider clients, retry policy, request schemas, or introduce a new provider framework.

### Finding C — M006 never reached strict closure on the final consolidated tree

Severity: medium planning/evidence defect.

Current repository evidence:

- `plans/closure/runtime-consolidation-deletion-footprint/006-status.md` is still `blocked`;
- its default and production-feature release measurements were taken before the final M003 physical extraction;
- that closure record explicitly requires a repeat measurement after M003 closes;
- the current registry nevertheless lists M006 as ready while M007 has already been moved to conditional closure.

M006 must be rerun after Findings A and B are corrected, because those changes define the actual final production tree.

### Finding D — M007 was advanced before its hard predecessor and evidence requirements completed

Severity: medium planning/evidence defect.

Current repository evidence at the reviewed baseline:

- M007's implementation plan requires M001–M006 to have accepted closure records before strict closure;
- the provisional `007-status.md` was created while M006 was not closed;
- the provisional record states that the production-feature release measurement did not finish;
- the provisional record was written while the cited ordinary hosted CI run had not yet reached a final conclusion;
- the local broad workspace test attempt also did not complete.

The provisional M007 record is useful historical evidence but cannot control strict closure. M009 must reconcile it after the exact final candidate exists.

### Finding E — planning metadata around M003/M008 is stale

Severity: low.

Current repository evidence:

- `003-agent-loop-ownership-decomposition.md` still says `corrective pass required` even though the corrective implementation landed;
- `008-m003-corrective-physical-extraction.md` still says `ready for handoff` even though commit `0dae4d8c` implemented the pass;
- `003-status.md` still refers to the corrective implementation commit as pending rather than recording `0dae4d8ce9a7988aef3b11db5ffa8b5993722712`.

M009 must reconcile those current-state planning fields without rewriting historical failure evidence.

## 3. Why the original verification did not catch these gaps

### M001 verification gap

The M001 regression coverage proved that the legacy `Task*` requests were explicitly rejected. It did not include a vertical TUI-to-daemon scheduling test proving that the user-visible task commands still created/listed/deleted durable schedules. The test therefore validated the compatibility rejection mechanism while missing the live caller that still depended on it.

Corrective requirement: add regression evidence at the consumer boundary, not another source scanner.

### M003 verification gap

The M003 focused loop/recovery/harness tests validated behavioral equivalence and the newly introduced module seams. The first pass was correctly identified as façade-only and received a physical extraction corrective pass, but that corrective pass closed after extracting the largest context/tool bodies without applying the same physical-ownership criterion to provider streaming/retry.

Corrective requirement: source ownership must be inspected directly in addition to behavior tests; `provider_turn.rs` must own the provider-turn implementation body rather than only call back into a loop-owned implementation.

### M006/M007 planning gap

M006 itself truthfully recorded that it was blocked and required rerun. The defect was later registry/closure sequencing: M007 was advanced using provisional evidence before the hard predecessor was strictly closed.

Corrective requirement: M009 is the controlling pass. No registry or roadmap state may mark M006/M007 closed until the required final-tree evidence exists.

## 4. Explicit non-goals

Do not:

- restore `BackgroundScheduler`, `BackgroundTask`, the deleted timer loop, or any second scheduling persistence model;
- add a new task/schedule schema or third identifier type;
- remove the retained legacy `Task*` wire variants merely to simplify this pass; they may remain explicitly unsupported for old external callers;
- redesign schedule recurrence syntax, scheduler admission, job attempts, leases, retry, or resource governance;
- redesign TUI command names or user-facing task UX beyond what is needed to restore existing behavior;
- rewrite provider network clients or provider-specific serializers;
- alter provider retry/backoff semantics merely because the code moves modules;
- introduce provider middleware, actor systems, service locators, dependency-injection frameworks, or new generic traits;
- continue broad `AgentLoop` decomposition beyond the named provider-turn body;
- reopen M002 structured recovery or M004 PromptCompiler/history work without new failing evidence;
- force dependency upgrades, binary reductions, or `panic = "abort"` to manufacture a footprint delta;
- add any CI workflow, lane, matrix, scheduled audit, coverage/benchmark/size gate, dependency bot, artifact workflow, release automation, workflow-dispatch mechanism, or fixed release cadence;
- add a static scanner just to verify this corrective pass.

## 5. Invariants that cannot regress

Scheduling and compatibility:

- the durable schedule/job infrastructure remains the sole production scheduling authority;
- no UUID-string-to-`u64` scheduling bridge is reintroduced;
- TUI task operations use durable schedule identities returned by the authoritative API;
- restart/recovery semantics remain owned by durable schedule/job storage and scheduler services;
- no TUI path directly dispatches work around daemon/scheduler authority.

Agent/provider runtime:

- `AgentLoop` remains the turn orchestration owner, not the provider transport implementation owner;
- the provider adapter preserves existing normalized `ChatEvent` ordering, retry limits, stop semantics, cancellation, and errors;
- provider protocol repair remains separate from M002 semantic recovery;
- structured tool outcomes remain typed and are not downgraded to strings because code moves;
- no new model-family heuristic is introduced in generic loop orchestration.

Verification and release:

- M006 measurements describe the exact post-correction tree;
- M007 hosted evidence describes the exact final candidate, not an earlier implementation commit;
- routine CI remains one bounded job;
- release remains manual;
- closure records preserve unsuccessful/incomplete predecessor evidence rather than rewriting history.

## 6. Ordered work packages

### A. Rebase and reconfirm exact current state

Before editing production code:

1. inspect current `main` and record the exact implementation baseline;
2. verify the active TUI still routes task commands through `TaskSchedule`, `TaskList`, and `TaskDelete`;
3. inspect the current durable `ScheduleCreate`, `ScheduleList`, and `ScheduleDelete` request/response DTOs and daemon handlers;
4. verify `BackgroundScheduler`/`src/agent/task.rs` remain absent;
5. inspect `provider_turn.rs` and locate the complete current `stream_with_retry_impl` body and every caller;
6. inspect the current M003/M006/M007 status records and registry before changing planning state;
7. if repository reality has already independently fixed any finding, preserve that work and narrow M009 accordingly rather than reimplementing it.

### B. Restore TUI scheduling through the durable API

Use the existing durable schedule protocol/service as the only target.

1. Replace the active TUI handlers' legacy request construction with the corresponding durable schedule request variants.
2. Map existing TUI recurrence input (`interval_secs`) onto the existing durable schedule representation using the repository's canonical parser/DTO rather than inventing another grammar.
3. Preserve session/project/workspace context required by the durable schedule API. Do not infer durable identity from the process CWD.
4. Map durable create/list/delete responses back to the existing TUI presentation with the smallest compatibility conversion necessary.
5. Display durable schedule identifiers consistently; do not parse a UUID/string through `u64` unless the durable protocol's actual typed identifier is numeric by contract.
6. Remove or consolidate dead duplicate TUI helper functions and `#[allow(dead_code)]` annotations that become unnecessary as part of this exact path, but do not broaden into TUI cleanup.
7. Leave legacy `CoreRequest::Task*` variants explicitly unsupported unless a separate supported external compatibility requirement is proven. The TUI must simply stop using them.

Required behavioral regression coverage:

- a TUI schedule action reaches `ScheduleCreate`/durable creation and returns a usable ID;
- list uses durable schedule records and displays the created schedule;
- delete removes the durable schedule and the next list no longer contains it;
- restart/persistence semantics are demonstrated by an existing durable-schedule integration test or one narrow extension if no current test covers it;
- no active TUI task handler constructs `TaskSchedule`, `TaskList`, or `TaskDelete` after the migration;
- the legacy request rejection test remains valid for old external callers.

Prefer a daemon/protocol integration test or a narrow TUI command-client test over a source-text scanner.

### C. Finish provider-turn physical ownership extraction

Move only the remaining provider-turn implementation that is currently hidden behind `ProviderTurnAdapter::receive()` calling back into `AgentLoop`.

1. Inventory `stream_with_retry_impl` and helper functions it directly requires.
2. Move the provider streaming/retry/event-normalization body into `src/agent/provider_turn.rs` or another already-existing provider-turn owner.
3. Delete the loop-owned implementation body after the move.
4. It is acceptable for the provider-turn owner to borrow/access the minimum `AgentLoop` state needed during this corrective pass if avoiding that borrow would require a larger state-container refactor. The important requirement is physical policy ownership, not a new abstraction hierarchy.
5. Keep retry count/backoff, cancellation, timeout, provider errors, normalized events, usage, and existing tracing semantics behaviorally identical unless a focused test proves an existing defect.
6. Do not move tool recovery, context packing, plugin hooks, goal state, or unrelated turn orchestration into the provider module.
7. Generic `run_inner` should call the provider-turn owner and consume normalized events; it should not contain the relocated retry/stream implementation.

Required regression coverage:

- existing provider stream retry tests remain green;
- agent loop harness remains green;
- cancellation/error behavior touched by the move remains green;
- no second provider retry owner is introduced;
- direct source inspection shows `provider_turn.rs` contains the implementation body and no façade call immediately delegates it back to a loop-owned equivalent.

### D. Reconcile M003/M008 status truthfully

After work package C is green:

1. change the M003 implementation-plan status from stale `corrective pass required` to the repository's implemented/closed-plan convention;
2. change plan 008 from stale `ready for handoff` to implemented/closed/superseded-by-closure according to existing convention;
3. update `003-status.md` to record commit `0dae4d8ce9a7988aef3b11db5ffa8b5993722712` as the corrective physical-extraction commit and then add the M009 provider-turn corrective commit when known;
4. do not erase the fact that the original M003 pass was incomplete and required plan 008;
5. update `architecture/agent.md` only where the final provider-turn ownership description would otherwise be false.

### E. Rerun and strictly dispose M006 on the final production tree

Only after work packages B and C are committed/settled:

1. rerun `cargo tree -e features --locked`;
2. rerun `cargo tree -d --workspace --locked`;
3. rerun the direct-dependency reachability review if production imports changed;
4. build and record the default release binary using an isolated target directory;
5. build and record the documented production feature combination using an isolated target directory;
6. use `cargo bloat` only if locally available and useful; it remains diagnostic only;
7. compare against the recorded pre-corrective/post-M005 values without claiming a reduction that measurement does not show;
8. do not make dependency/feature/profile changes unless the final tree exposes a clear, low-risk, no-feature-loss cleanup under the original M006 criteria;
9. update `006-status.md` from `blocked` to `closed` only when all original M006 acceptance criteria are actually satisfied on the final consolidated tree;
10. if the production-feature release build cannot complete in the available environment, leave M006 blocked/conditional and stop strict M007 closure rather than substituting an older number.

Expected measurements remain evidence only. No size threshold is introduced.

### F. Final broad verification and hosted evidence

Once work packages B–E are complete, run the existing M007 contract on the exact candidate.

Required local/focused evidence:

```bash
cargo fmt --all -- --check
cargo check -p codegg --locked
cargo test -p codegg-core jobs::schedule -- --nocapture
cargo test -p codegg --lib agent::progress_recovery -- --nocapture
cargo test -p codegg --lib agent::r#loop::tests -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
scripts/verify.sh quick
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check -p codegg --locked --features server,plugins,lsp-test-support
git diff --check
```

Add the focused TUI/durable-schedule regression test command selected in work package B and the provider-turn focused command selected in work package C.

Broad workspace evidence:

- use the repository's existing workspace-test contract once on the final candidate;
- if a known external-LSP-dependent test blocks locally, record the exact test and use the ordinary hosted CI run as the authoritative existing CI contract rather than adding another lane or special workflow;
- do not silently omit a failing in-scope test.

Hosted evidence:

- push the exact candidate normally;
- record one ordinary existing `CI / verify` run ID and job ID;
- wait for a final conclusion in the implementation session only as normal command/tool execution permits; do not create a new workflow to obtain evidence;
- strict closure requires the existing hosted run to pass on the exact accepted candidate.

### G. Reconcile M007, roadmap, registry, and closure records

After all required evidence is available:

1. create `plans/closure/runtime-consolidation-deletion-footprint/009-status.md` with the corrective requirement-to-evidence matrix;
2. update `007-status.md` to distinguish the earlier provisional conditional evidence from the final accepted evidence; preserve the earlier incomplete measurement/CI history;
3. mark M006 closed only if work package E passed;
4. mark M007 and the subsystem roadmap closed only if all original M007 exit criteria and M009 corrective criteria pass;
5. if any critical/high/medium in-scope defect remains, keep the roadmap open and classify the exact blocker;
6. remove M009 from dependency-ready registry work after closure and move it to recently closed control points;
7. ensure M006/M007 are not simultaneously shown as ready/blocked/conditionally closed in contradictory registry sections;
8. do not touch unrelated provider/tool/DVR/runtime-safety planning status.

## 7. Storage, protocol, compatibility, and migration effects

Storage:

- no schema migration is expected;
- TUI scheduling must reuse existing durable schedule/job tables and stores;
- no legacy in-memory task persistence is restored.

Protocol:

- use the existing durable `Schedule*` request/response protocol;
- retained legacy `Task*` variants may remain wire-compatible but unsupported;
- no public wire shape change is expected.

Compatibility:

- user-visible TUI task schedule/list/delete behavior is restored;
- old external clients that still send legacy `Task*` requests continue to receive the existing explicit migration/unsupported response unless separately migrated;
- provider wire formats and provider-visible retry semantics remain unchanged by the physical move.

Migration:

- no operator action;
- no config migration;
- no background task data migration beyond the already-existing legacy-to-durable startup migration.

## 8. Concurrency, cancellation, restart, and failure semantics

Scheduling:

- concurrent create/list/delete remain governed by durable store/service semantics;
- no TUI-owned mutable schedule cache becomes authoritative;
- daemon restart rehydrates durable schedules through existing infrastructure;
- create failure must not produce a false-success TUI toast/ID;
- delete/list errors remain actionable and bounded.

Provider turn:

- moving code must not introduce a lock across provider awaits that did not previously exist;
- turn cancellation and provider stream cancellation retain existing propagation;
- retry budget and provider backoff remain bounded and unchanged;
- a provider error must remain a typed `ProviderError`/`AppError` path, not become semantic tool recovery;
- no detached background provider task may outlive the existing turn/session lifecycle because of the extraction.

Closure/evidence:

- incomplete measurements or in-progress hosted runs are not success;
- an unrelated hosted failure may support conditional closure only if the original M007 policy permits it and the closure record proves it is unrelated; an in-scope failure requires correction.

## 9. Security and authority review

The corrective pass must explicitly verify:

- the TUI cannot schedule work outside the session/project/workspace authority accepted by the durable schedule API;
- no scheduler bypass/direct process path is introduced;
- legacy `Task*` rejection cannot be used to reach removed background scheduler state;
- provider-turn relocation does not broaden tool permissions, child-agent authority, filesystem paths, provider credentials, or secret visibility;
- private reasoning remains excluded from public projection as before;
- existing sandbox, execution-ownership, Tool Broker, Git, and disclosure guards remain unchanged unless a directly affected annotation must move with code.

Do not add a new static guard for these properties unless the implementation discovers a genuinely unenforced critical invariant and documents why types/tests cannot cover it.

## 10. Documentation updates

Required current-state documentation updates are limited to:

- `architecture/agent.md` for final provider-turn ownership if necessary;
- `architecture/jobs.md` / `architecture/scheduler.md` if they currently imply the TUI uses rejected legacy task requests;
- M003/M006/M007/M009 planning and closure records;
- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`;
- `plans/registry.md`.

Do not add exact test counts, field inventories, or transient implementation mapping to architecture documents.

## 11. Explicit acceptance criteria

M009 is complete only when every applicable item is true:

1. `BackgroundScheduler`, `BackgroundTask`, and the deleted independent timer/persistence/dispatch implementation remain absent from production code.
2. No UUID-string-to-`u64` legacy scheduler dispatch bridge is reintroduced.
3. The active TUI task schedule path uses the durable schedule API, not `CoreRequest::TaskSchedule`.
4. The active TUI task list path uses the durable schedule API, not `CoreRequest::TaskList`.
5. The active TUI task delete path uses the durable schedule API, not `CoreRequest::TaskDelete`.
6. Schedule create/list/delete works through one daemon-owned durable source of truth and preserves required session/project/workspace authority.
7. Focused regression coverage proves a user-visible schedule can be created, listed, and deleted through the corrected TUI/client path.
8. The retained legacy `Task*` request rejection behavior remains deterministic for old external callers and does not create a second scheduler.
9. `provider_turn.rs` (or the already-existing provider-turn owner) contains the real streaming/retry/event-normalization implementation body; it is not merely a façade that immediately invokes a loop-owned `stream_with_retry_impl` equivalent.
10. Generic `AgentLoop` no longer owns that provider streaming/retry implementation body.
11. Provider retry, cancellation, timeout, normalized event ordering, stop reason, usage, and error behavior remain compatible under focused tests.
12. No new provider framework, retry owner, generic trait hierarchy, shared global state, or gratuitous `Arc<Mutex<...>>` is introduced.
13. M002 structured recovery and M004 PromptCompiler/history invariants remain green and unchanged.
14. M003 and plan 008 statuses/commit evidence reflect actual completed implementation without erasing their corrective history.
15. M006 feature-tree, duplicate-version, default release, and production-feature release measurements are rerun on the post-M009 production tree.
16. `006-status.md` is marked closed only if the original M006 acceptance criteria are satisfied on that final tree.
17. Final footprint evidence reports actual values and makes no unsupported size-reduction claim.
18. `scripts/verify.sh quick`, focused scheduling/provider/loop/recovery tests, workspace Clippy, production-feature compile, and `git diff --check` pass on the accepted final candidate.
19. The broad existing workspace verification contract is run once; any incomplete/external test is recorded truthfully rather than omitted.
20. One ordinary existing hosted `CI / verify` run passes on the exact final candidate before M007/roadmap strict closure.
21. No new CI lane, matrix, schedule, workflow dispatch, artifact workflow, size/coverage/benchmark gate, dependency bot, release automation, or fixed release cadence is added.
22. `009-status.md` records implementation commits, a requirement-to-evidence matrix, commands/results, compatibility/security findings, measurements, hosted run/job IDs, and unresolved findings by severity.
23. `007-status.md`, the subsystem roadmap, and `plans/registry.md` agree on the final disposition; M006/M007/M009 do not have contradictory active states.
24. No critical/high/medium in-scope finding remains when the roadmap is marked closed.

## 12. Stop conditions

Stop and record a blocker rather than expanding M009 if:

- the current durable schedule protocol cannot represent the existing TUI recurrence semantics without a public protocol decision;
- restoring TUI scheduling would require a new persistence model or scheduler implementation;
- provider-turn extraction requires a public provider API redesign rather than a physical ownership move;
- M006 production-feature measurement cannot complete reproducibly in the available environment;
- hosted CI fails on an in-scope production defect that requires more than a narrow correction;
- any proposed fix would weaken scheduler authority, workspace identity, permission enforcement, Tool Broker authority, private-reasoning isolation, or provider credential handling.

If a stop condition occurs, leave the roadmap active and identify the exact blocker. Do not manufacture strict closure through documentation-only status changes.

## 13. Required closure evidence

`plans/closure/runtime-consolidation-deletion-footprint/009-status.md` must contain:

- exact baseline and final candidate SHA;
- implementation commit(s);
- explicit disposition of Findings A–E;
- TUI durable-schedule create/list/delete evidence;
- proof that legacy scheduler code remains absent;
- provider-turn physical-ownership evidence;
- focused test commands/results;
- M006 final feature/dependency/size measurements;
- broad local verification result;
- hosted CI run/job/final conclusion on the exact candidate;
- planning/documentation reconciliation summary;
- storage/protocol/migration/security assessment;
- unresolved findings classified critical/high/medium/low/deferred;
- final recommendation: closed, conditionally closed, corrective pass required, or blocked.

Strict roadmap closure is accepted by M009 after M006's final-tree evidence and
M007's exact-candidate hosted verification are complete.
