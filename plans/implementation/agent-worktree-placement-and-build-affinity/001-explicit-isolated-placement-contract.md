# Agent Worktree Placement and Build Affinity A001 — Explicit Isolated Placement Contract

Status: **conditionally closed** — closure `plans/closure/agent-worktree-placement-and-build-affinity/001-status.md`; contained-creator live success remains open
Repository baseline: `80171f45295bebb9ab0b521262c0b295acb93074`
Source roadmap: `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md` A001
Long-term: `plans/000-long-term-specification.md` §§16–18, 26–29; `plans/001-terminology-and-domain-model.md`.
Accepted ADRs: ADR-0002, ADR-0032, ADR-0047, ADR-0048. Primary class: **invariant**.

## 1. Objective
An explicit `isolated_new` delegated placement always allocates a fresh managed worktree before child execution or fails with a typed reason — even when that child is read-only. `auto` retains the current read-only inherited-root optimization, and `serialized_shared` retains its independent restricted group checkout behavior.

## 2. Why ready
The durable WorktreeService, scheduler executor and group coordinator already exist and their relevant milestones are strictly closed. No authority expansion or ADR is needed to honor the intent already accepted by `RunPlacement`.

## 3. Evidence
`crates/codegg-core/src/run_placement.rs` documents `IsolatedNew` as explicit new checkout. `src/tool/task.rs` parses `isolated_new` and propagates per-child and group placement into `JobPayload::SubagentRun`. `src/scheduler/executors.rs` uses `needs_isolation || wants_shared_checkout` to enter worktree preparation, then chooses the isolated arm only if `needs_isolation`. For a read-only profile with `IsolatedNew`, neither is necessarily true. This is a source-level hypothesis requiring a reproduced baseline failure, not a claimed executed test. Non-repo `auto` currently has a deliberate in-place compatibility route; explicit isolation must not silently use that route.

## 4. Invariants
One distinct worktree per explicitly isolated child, matching repository lineage and immutable base; lease generation / cleanup policy unchanged; no permission expansion for a read-only child; original `Auto` and serialized shared behavior preserved; no manual worktree adoption, no parent-branch automatic merge, no containment bypass.

## 5. Scope
In: placement interpretation at task/scheduler boundary, no-repository and unavailable-host negative behavior, projection truth and tests. Out: existing-checkout selection (A002), shared writes/process restrictions, cache/build admission, default changes, new Git backend.

## 6. Production changes
Core/protocol: retain enum and serialized representation. Executor: explicit predicate for `IsolatedNew`, distinct from inferred mutation and shared placement, preparing a new worktree even if permissions are read-only. Tool: prove `spawn` and `spawn_many` attach the requested placement. Diagnostics: differentiate explicit isolation unavailable from auto-in-place and list the reason; make false-success impossible.

## 7. Work packages
A. Write red test: read-only delegated `isolated_new` under real scheduler + managed WorktreeService; assert distinct linked path before tool execution. B. Correct predicate and fails-closed checks, including repository-less and missing-contained-backend routes. C. Test `auto` read-only inherited path, `auto` mutating isolated path, `serialized_shared` group-owned path, mixed group per-child overrides and retry/continuation. D. Audit projections and docs.

## 8. Failure/restart/concurrency
Failure in preparation produces a terminal refused run, not a silently rerooted child; keep unclean worktrees and generation-fenced leases. Concurrent identical retry converges by durable idempotency. A canceled child cannot leave an active lease or clean up another child's directory.

## 9. Compatibility
Unchanged `serde(default)` means legacy jobs continue to decode `Auto`; no migration or new placement variant. Read-only `isolated_new` intentionally gains the promised new-checkout overhead.

## 10. Tests
Real scheduler and worker role/capability matrix; nested child; retry, cancel, old payload, invalid ref, detached checkout, host-without-containment. Test both live production dispatch and helper logic (unit-only coverage is not closure).

## 11. Verification
```bash
cargo test -p codegg-core --lib run_placement
cargo test -p codegg --lib admission_tests
cargo test --test scheduler_jobs -- scheduler_contention
cargo test --test agent_loop -- blackboard_trajectory_m008
python3 scripts/check_shared_checkout.py --self-test
python3 scripts/check_shared_checkout.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets -- -D warnings
```
Validate selected test count against current family names. These are **commands to run**, not work claimed performed.

## 12. Documentation
Update `architecture/worktree.md`, `architecture/writer_permit.md`, `docs/tools.md` and the model-facing task schema description if needed.

## 13. Acceptance
Two explicit isolated read-only children have two distinct managed roots; neither touches parent; Auto read-only uses no extra checkout; explicit isolated non-Git run refuses with actionable error; shared grouping is unchanged.

## 14. Stop
Any change that shares mutable checkout implicitly, treats paths as identity, suppresses security/backend refusal, broadens agent permissions or rewrites accepted ADRs.

## 15. Closure evidence
Baseline failing red test, precise fix/commit, live scheduler tests, negative cases, completed verification, run worktree records and lease dispositions, OS qualification labels.

## 16. Handoff
Small correctness patch; do not bundle A002's new placement variant or A003 build resource policy. Close A001 before marking A002 ready.
