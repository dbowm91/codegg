# Post-Audit Maintainability and Surface Milestone 003 — Agent-Runtime Physical Decomposition

Status: ready for handoff

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Source roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Soft dependency:

- M001 may proceed in parallel, but implementations must coordinate edits to shared agent tool-registration/prompt regions.

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: polish

## 1. Objective

Reduce maintenance concentration in `src/agent/loop.rs` and `src/agent/mod.rs` by moving coherent existing responsibilities into named modules with focused tests, while preserving the current `AgentLoop` orchestration object, daemon/scheduler authority, runtime state machines, public behavior, and existing subsystem ownership.

This milestone is a physical/source-organization refactor. It must improve locality and testability without introducing another coordinator, another state machine, or a generic abstraction layer whose only purpose is reducing file size.

## 2. Why this milestone is ready

The architecture-convergence work has already done the difficult ownership reduction. The audit baseline shows the remaining problem is physical concentration inside canonical implementations:

- `src/agent/loop.rs` is approximately 203 KiB;
- `src/agent/mod.rs` is approximately 132 KiB;
- surrounding modules already own substantial stable responsibilities, including context runtime, asset snapshots/refresh, convergence, coordinator behavior, prompts, progress recovery, tool batching/surface, and delegation/run support.

Because ownership is already mostly explicit, coherent functions/types can now be moved to existing or narrowly new modules without choosing a new architecture.

M001 is only a soft dependency. If M003 is implemented concurrently, avoid moving compatibility/tool-surface code that M001 is actively editing, or land one plan first and rebase the other.

## 3. Current implementation evidence

The implementer must first create a responsibility map of the two files, based on current function/type groups rather than line count alone. At minimum identify code belonging to:

- `AgentLoop` construction/configuration and immutable runtime dependencies;
- turn lifecycle and model-call loop;
- prompt/message/context construction;
- tool-call parsing, batching, broker execution, projection, and result incorporation;
- compaction/context lifecycle triggers;
- delegation/subagent/team coordination and joins;
- progress/no-progress/replan integration;
- run/goal completion and verification;
- cancellation/shutdown paths;
- test fixtures/helper types currently co-located with production code;
- broad `agent::mod` exports, built-in agent composition, and compatibility glue.

Existing surrounding modules are evidence that the repository already favors responsibility-oriented extraction. Prefer extending those boundaries when they fit over creating nearly synonymous new modules.

The milestone should measure source movement in terms of responsibility and dependency direction. A smaller file that merely re-exports several giant helper modules with circular state access is not success.

## 4. Invariants that must not regress

- `AgentLoop` remains the canonical per-turn orchestration object unless an accepted architecture document already says otherwise.
- Daemon/scheduler admission remains authoritative for heavy/durable execution.
- Tool execution remains broker/contract/policy governed; module extraction cannot create direct bypass calls.
- Turn-start runtime asset snapshots remain immutable for the active turn.
- Context compaction remains on its accepted ownership path and does not acquire a second scheduler/state machine.
- Child-agent effective authority cannot exceed parent/session/project/workspace/tool policy.
- Cancellation continues to propagate through child runs/jobs/processes according to existing semantics.
- Worktree/run/session attribution and correlation IDs remain unchanged.
- Projection/event ordering and boundedness remain unchanged.
- Model adaptation/profile/tool-surface behavior remains unchanged except changes separately owned by M001/M002.
- Public APIs are not renamed merely to simplify module moves.
- No new trait hierarchy is introduced without at least two real implementations or a clear existing boundary need.

## 5. Scope

### In scope

- Build a responsibility map and dependency graph for the two oversized files.
- Extract coherent production sections into existing modules where ownership already matches.
- Create narrowly named modules where no existing owner fits.
- Move associated focused tests beside the extracted responsibility when practical.
- Reduce broad `use super::*`/module-private coupling and make inputs/outputs explicit.
- Tighten visibility (`pub(crate)`, private) where moving code makes unnecessary public exposure apparent, provided no supported public API is broken.
- Separate test-only fixtures/helpers from production implementation where that materially improves navigation/compile boundaries.
- Update agent architecture documentation when module ownership descriptions change.

### Explicitly out of scope

- Rewriting the agent loop algorithm.
- Changing model-call retry/convergence semantics.
- New agent types or subagent features.
- Tool-surface minimization (M002).
- Runtime global-state cleanup (M005), except accepting an interface seam that M005 can later consume.
- Moving arbitrary code into `codegg-core` merely to shrink root files.
- Introducing an event-sourcing rewrite, actor model, service bus, or generalized workflow engine.
- Splitting files to satisfy a hard line-count limit.

## 6. Required production changes

### Core/domain

Start from behavior clusters and stable state ownership. The expected extraction shape is approximately:

```text
agent/
  loop.rs                 # top-level turn loop/orchestration and high-level sequencing
  turn_runtime.rs         # turn-local state/lifecycle if not already owned elsewhere
  tool_execution.rs       # model-call tool batch -> broker -> projection/result integration
  completion.rs           # root completion/goal verification/join disposition where coherent
  delegation/...          # existing child coordination owners, extended rather than duplicated
  context_runtime.rs      # existing owner retained for context/compaction responsibilities
  ...
```

These names are illustrative. The implementation agent must inspect existing modules and avoid creating duplicates such as `tool_execution.rs` if `tool_batch.rs` already owns the exact responsibility.

For `src/agent/mod.rs`, move implementation families out of the module root while keeping its role as module declaration/re-export/composition surface. Built-in agent registration/composition, helper runtime construction, or test suites may deserve separate existing/new modules if they form coherent units.

### Storage and migrations

No storage migration is expected. SQL/run-store calls must remain on the same semantic path with unchanged IDs/transactions.

### Protocol and DTOs

No wire change is expected. Extract serialization/projection conversion helpers without altering field names/order/version semantics.

### Runtime and concurrency

Be explicit about ownership when moving async code:

- which task owns cancellation tokens;
- which futures may outlive a model response;
- which locks guard turn/session state;
- where scheduler permits/submissions are acquired;
- where child runs are joined/cancelled;
- where provider streams are polled and aborted.

Do not clone large mutable state into helper structs merely to avoid parameter lists. Prefer narrow references/typed contexts around already accepted runtime state.

### Frontend or operator surface

No behavior change expected. TUI/ACP/native projections should be byte/semantic-equivalent except incidental debug/source-location text.

### Security and authorization

Moving tool/delegation functions must not bypass authority intersection or permission checks. If code presently mixes permission construction with execution, extraction should make the boundary clearer but not duplicate it.

### Documentation and static guards

Update `architecture/agent.md`, `architecture/agent-tool-surface.md`, or current agent architecture index to reflect final module ownership. Do not add a file-size CI gate.

## 7. Ordered work packages

### Work package A — Responsibility and dependency map

Intent: prevent mechanical splitting.

Required actions:

1. Inventory top-level types/impl blocks/helpers/tests in `loop.rs` and `mod.rs`.
2. Assign each to an existing canonical responsibility where possible.
3. Mark shared state each group reads/writes and its async/cancellation dependencies.
4. Identify 2–5 extraction units that reduce unrelated coupling and can be verified independently.
5. Identify code that must remain in the orchestrator because it is high-level sequencing rather than a reusable responsibility.

Acceptance evidence:

- concise extraction map in implementation notes/closure;
- no proposed module duplicates an existing owner.

### Work package B — Extract low-risk pure/typed helpers first

Intent: establish dependency direction before async/lifecycle movement.

Required changes:

- move parsing/projection/classification/turn-state helpers with narrow inputs;
- move their tests;
- replace broad module imports with explicit dependencies.

Acceptance evidence:

- focused helper tests pass;
- no public behavior/API change.

### Work package C — Extract one or more lifecycle/execution responsibility clusters

Intent: remove the largest maintenance concentration while preserving state ownership.

Required changes:

- move selected tool-execution, completion, or delegation/context integration clusters according to WP-A;
- pass explicit existing runtime context/state references;
- preserve task cancellation and scheduler/broker boundaries.

Acceptance evidence:

- focused agent runtime/tool/delegation/context tests pass;
- no new direct provider/tool/process execution path appears.

### Work package D — Reduce `agent/mod.rs` to composition/export responsibility

Intent: make module root navigational rather than another implementation monolith.

Required changes:

- relocate implementation/test families that have stable ownership;
- keep intentional re-exports and module declarations concise;
- tighten visibility where safe.

Acceptance evidence:

- downstream workspace imports compile unchanged or with intentional internal-path migration;
- supported public paths remain unless separately approved.

### Work package E — Reconcile docs and inspect dependency direction

Intent: ensure the refactor actually improves architecture readability.

Required actions:

- inspect for new circular imports/pass-through modules;
- update architecture docs;
- record before/after file sizes only as descriptive evidence, not an acceptance gate.

Acceptance evidence:

- each new/existing module has one describable responsibility;
- high-level `AgentLoop` sequencing can be followed without reading unrelated implementation families.

## 8. Failure, cancellation, restart, and contention semantics

This milestone is behavior-preserving, so closure must explicitly confirm the existing semantics rather than invent new ones.

A moved async operation must retain the same cancellation token and parent lifetime. Avoid detached `tokio::spawn` introduced purely to simplify ownership/borrow issues. If extraction makes a task need to be detached where it was previously joined, stop: that is a behavior change.

Restart/replay persistence must use the same run/session/attempt identifiers and stores. No moved helper may derive identity from a path or fresh random value when the original code consumed typed context.

Lock acquisition ordering must not change casually during extraction. If a borrow workaround changes when a lock is held across `.await`, add a focused contention/deadlock regression and document why the new ordering is safer.

## 9. Compatibility and migration

No user migration is expected.

Internal Rust paths may change within the crate. Preserve documented/re-exported downstream paths unless M001 has independently dispositioned them. If a type/function is public only accidentally, tightening visibility is allowed only after a repository/downstream evidence search.

Do not modify durable serialized tool/run/session names.

## 10. Required tests

### Focused unit tests

Move existing tests with extracted helpers and add only tests needed to preserve a newly explicit boundary.

### Integration tests

At minimum select existing coverage for:

- ordinary turn with one/multiple tool calls;
- tool batching/broker path;
- compaction/context lifecycle;
- child delegation/join/cancel;
- completion/goal verification;
- provider stream/error handling;
- projection/event delivery.

### Restart and recovery tests

Run existing durable agent run/rerun/replay tests if touched code participates in persistence/recovery.

### Contention and cancellation tests

Run existing cancellation, child-run, tool-batch, scheduler, and lock/contention coverage for touched code. Add a regression only if extraction changes lock boundaries.

### Security and negative tests

Run permission/authority/child-delegation negative tests for moved execution/delegation code.

### Migration and compatibility tests

Only if public module paths/re-exports change.

## 11. Required verification commands

Use actual current test selectors after the responsibility map. Expected commands include:

```bash
cargo test -p codegg agent::
cargo test -p codegg --test '*agent*'
cargo test -p codegg --test '*tool*'

# ownership guards relevant to execution moves
python3 scripts/check_execution_ownership.py
./scripts/check-core-boundary.sh

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not run globs literally if Cargo does not accept them; select current named tests/packages discovered at implementation time.

## 12. Documentation updates

- Agent architecture/module ownership documentation.
- `architecture/tool.md` only if agent-side broker integration descriptions move materially.
- `AGENTS.md` source-layout notes if they name obsolete giant-file locations.
- No user README change unless public paths/behavior are affected.

## 13. Acceptance criteria

- A responsibility map justified the extraction before code movement.
- `loop.rs` and `mod.rs` are materially less physically concentrated, with coherent responsibility modules carrying the moved code/tests.
- `AgentLoop` high-level sequencing remains recognizable and canonical.
- No new coordinator, state machine, provider path, tool executor, scheduler, or generic workflow abstraction is introduced.
- Cancellation, context/compaction, delegation, goal/completion, persistence, and projection semantics remain covered and green.
- Dependency direction is no worse: new modules do not form pass-through/circular ownership webs.
- Public compatibility is preserved unless independently approved.

## 14. Stop conditions

Stop and report when:

- extraction requires changing the canonical owner of a subsystem;
- borrow/lifetime pressure appears solvable only by detaching tasks or duplicating mutable state;
- a proposed helper trait has only one implementation and no real contract benefit;
- moving code into `codegg-core` would introduce forbidden root-runtime dependencies;
- M001 concurrently edits the same code and safe rebase/coordination is not possible;
- broad agent behavior starts changing beyond source organization;
- current head materially differs from the baseline responsibility map.

## 15. Closure evidence required

- implementation commits/PRs;
- before/after responsibility map and descriptive file sizes;
- list of extracted modules and one-sentence ownership for each;
- evidence no new direct execution/provider/tool bypass was introduced;
- focused agent/tool/context/delegation/cancellation tests and outcomes;
- static ownership guard outcomes;
- formatting/lint/quick verification outcomes;
- public API/re-export compatibility disposition if changed;
- known remaining concentration and why it stays in the orchestrator.

## 16. Handoff notes

A large file is not automatically bad, and a small file is not automatically maintainable. Move only code that has a stable conceptual owner and test boundary.

Prefer three coherent extractions over twenty tiny files. The prior convergence work already paid the architectural cost of establishing ownership; do not recreate that cost under a new naming scheme.
