# Architecture Convergence M009 — Strict Closure Evidence and Guard Triage

Status: ready

Repository baseline: `1b0f8d076c71b5783769d9dae2b4efd5afa1d047`

Source corrective addendum:

- `plans/subsystems/architecture-convergence-strict-closure-corrective-addendum.md`

Source historical roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Relevant historical closure records:

- `plans/closure/architecture-convergence-incomplete-verticals/001-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/002-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/003-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/004-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/006-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/008-status.md`

Planning authority:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md`

Primary class: corrective verification / closure / maintenance triage

## 1. Objective

Reconcile the architecture-convergence workstream with CodeGG's status semantics by resolving or precisely retaining the remaining M001-M004 and M006 conditional-closure evidence, then triage the three pre-existing guard findings surfaced during M008.

This is a closure pass, not a new architecture pass. Production behavior from M001-M008 is presumed correct unless current evidence demonstrates otherwise. Prefer evidence and narrow fixes over refactoring.

## 2. Explicit non-goals

Do not:

- add new product features;
- redesign compaction, process execution, Git ownership, AgentLoop, rerun, command routing, LSP mutation, or projections without a demonstrated correctness defect;
- create new CI lanes, scanners, coverage/benchmark/binary-size gates, dependency bots, release automation, or a new verification framework;
- absorb the unrelated runtime-safety C002 Landlock evidence condition;
- broadly clean all `current_dir`, project-catalog, or tool-broker findings outside the exact sites reported by current guards;
- weaken, ignore, delete, or suppress a valid test/guard merely to claim closure;
- rewrite historical M001-M008 closure records.

## 3. Current implementation evidence to revalidate

At the reviewed baseline:

- M001 production compaction ownership is complete but the focused root compaction runtime binary was blocked by the local mixed-architecture linker environment.
- M002 production finite-process ownership is complete but focused runtime evidence and strict feature-heavy Clippy completion were locally blocked.
- M003 Git ownership convergence is complete; leaf-crate/core tests passed, while root focused execution and strict all-features Clippy had environmental/unrelated blockers.
- M004 AgentLoop coordinator reduction is complete; hosted testing passed M004-scoped coverage and thousands of root tests but reproduced `shell::runtime::tests::runtime_timeout_emits_timed_out_event` as an unrelated failure.
- M006 command-pipeline convergence is complete; compile/static/quick checks passed, local focused runtime linking was blocked, and its closure record was written before hosted workspace testing fully completed.
- M008 broad verification surfaced three low-severity pre-existing guard findings: daemon cwd usage, project-catalog invariant drift, and direct ReviewTool execution.

The implementing agent MUST inspect current `main` before assuming any of these findings remain.

## 4. Invariants that cannot regress

The corrective pass must preserve:

- daemon ownership of execution and durable state;
- scheduler ownership of admission/resource control;
- one canonical CodeGG context/compaction policy owner;
- one canonical finite-process lifecycle owner with explicit protocol-specialized exceptions;
- typed Git ownership and hardened environment/redaction boundaries;
- AgentLoop as a coordinator over typed services, not a duplicate policy owner;
- rerun as creation of a new child run with immutable parent history and no persisted raw secrets;
- one semantic command planning/dispatch pipeline;
- checked mutation/history authority for LSP-applied edits;
- frontend-neutral projection semantics and bounded/non-authoritative legacy transports;
- typed workspace/project/session/run identity;
- child-agent authority no wider than parent authority.

## 5. Work package 1 — Rebuild the conditional-closure matrix from current evidence

Before changing code:

1. Read M001, M002, M003, M004, and M006 closure records in full.
2. Extract every still-named condition into a working matrix with:
   - milestone;
   - exact command/test/evidence originally missing;
   - reason it was missing;
   - whether later commits or hosted runs already provide equivalent or stronger direct evidence;
   - whether the condition still applies to current `main`.
3. Do not treat a broad green command as superseding a focused condition unless it actually exercises the same invariant.
4. Record stale conditions explicitly if later accepted evidence already resolves them.

Deliverable: a current condition matrix suitable for inclusion in the M009 closure record.

Stop condition: if a material production correctness defect is discovered in any M001-M008 ownership boundary, stop broad closure work and register a separate corrective implementation plan for that defect unless the fix is demonstrably small and local enough to remain within M009.

## 6. Work package 2 — Resolve M001/M002/M003/M006 evidence with existing verification only

Use the repository's existing supported environment and existing commands. The exact historical command strings may have drifted; prefer current equivalent commands when documented by the owning closure record or current repository tooling.

Required minimum intent:

### M001

Exercise the canonical compaction path sufficiently to cover:

- provider/session request context propagation;
- cancellation returning structured non-mutating results;
- history/tool-call invariants;
- reserved-budget/capacity handling.

Prefer the existing focused compaction suite. Do not add a parallel harness if the existing test binary now runs normally.

### M002

Exercise the canonical managed-process path sufficiently to cover:

- timeout and cancellation;
- process-tree cleanup/reaping;
- bounded capture/streaming;
- shell/runtime integration used by the migrated callers;
- existing execution-ownership and sandbox guards.

Run the existing strict workspace Clippy command if current repository conventions still require it for closure.

### M003

Exercise current focused Git ownership coverage sufficient to confirm:

- canonical environment/process construction;
- root mutation-policy integration;
- worktree/lineage consumers;
- direct production `git` construction guard.

Do not broaden into network or worktree redesign.

### M006

Exercise the canonical command pipeline sufficiently to confirm:

- parse/normalize -> intent -> plan -> typed dispatch target;
- Git risk-family mapping;
- raw shell vs managed argv separation;
- active-routing authorization handoff;
- no independent production routing policy remains.

If current hosted/current-head evidence already directly covers a named historical condition, cite it rather than rerunning redundant heavy work.

## 7. Work package 3 — Reproduce and disposition the M004 shell timeout failure

The prior hosted run reproduced:

`src/shell/runtime.rs` test `shell::runtime::tests::runtime_timeout_emits_timed_out_event`

Required sequence:

1. Run the exact focused test on current head in a compatible environment.
2. If it passes repeatedly and current broader tests are green, record the old failure as resolved by later/current evidence.
3. If it still fails, determine whether the cause is:
   - deterministic production defect;
   - timing-sensitive/flaky test assumption;
   - host-specific scheduling behavior;
   - consequence of M002 managed-process migration;
   - unrelated pre-existing shell-runtime behavior.
4. If a fix is small and obviously owned by the current managed-process/shell boundary, implement the smallest correction with a focused regression test.
5. If correction requires broader shell/process redesign, do not absorb it. Register a separate corrective plan and leave architecture convergence `conditionally closed` until ownership is explicit.

No arbitrary sleep inflation. Any timing change must reflect a documented contract or deterministic synchronization condition.

## 8. Work package 4 — Triage the three M008 guard findings

Re-run the current guards first; do not work from stale M008 output alone.

### 8.1 Daemon cwd usage

Guard: `check_daemon_cwd_usage.py`

For each reported `current_dir` site:

- determine whether it is production daemon authority, a standalone script/tool path, test-only code, or a false positive;
- verify whether typed workspace/project context should be supplied instead;
- fix only if the current site violates the repository's daemon/path ownership invariant;
- otherwise update the guard manifest/documentation only if the exception is legitimate and currently undocumented.

Do not perform a repository-wide cwd refactor.

### 8.2 Project catalog invariant drift

Guard: `check_project_catalog_invariants.py`

Determine whether the failure represents:

- stale guard expectation after an accepted storage-layout change;
- actual project-catalog storage drift;
- a test fixture/docs mismatch.

If it is expectation drift, update the guard and architecture documentation to the accepted canonical layout. If it is a real project-catalog defect requiring migration or nontrivial storage changes, register a separate project-catalog corrective plan rather than expanding M009.

### 8.3 Direct ReviewTool execution

Guard: `check_tool_broker_boundary.py`

Trace the reported ReviewTool call from model-facing or agent-facing entry point through authorization, scheduler/admission if applicable, tool broker/backend, process/tool execution, persistence, and result projection.

Classify the path as:

- an intentional typed in-process tool execution that does not bypass an authority boundary;
- a guard false positive / allowlist gap;
- a true bypass of the canonical broker/backend/authorization path.

If it is a true bypass and the correction is a local rewire through the existing canonical tool boundary, fix it in M009 and add or preserve focused regression coverage. If fixing it requires a new broker abstraction or generalized tool-runtime redesign, register a separate corrective plan.

The handoff MUST document why the chosen classification is correct; merely adding an allowlist entry is insufficient.

## 9. Work package 5 — Planning and documentation reconciliation

After evidence/fixes:

1. Add `plans/closure/architecture-convergence-incomplete-verticals/009-status.md`.
2. Do not edit predecessor closure records except for non-substantive link corrections if absolutely necessary; historical findings must remain visible.
3. Update this plan to `implemented` only when production/evidence work is complete.
4. Update the corrective addendum milestone status.
5. Update `plans/registry.md` according to the final disposition:
   - `closed` only if no architecture-convergence condition remains;
   - `conditionally closed` if a named architecture-convergence condition remains;
   - `blocked` only if an external evidence requirement truly prevents further progress;
   - register any new separately owned corrective plan only if M009 found a real defect outside its bounded scope.
6. Keep the registry compact; do not duplicate the full M009 evidence matrix there.

## 10. Verification posture

Verification must remain minimal and evidence-driven.

Use focused commands appropriate to any changed code plus the repository's existing broad posture:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Also run the existing focused suites/guards needed to close the historical conditions and triage findings, including as applicable:

```text
cargo test -p codegg --test compaction
cargo test -p codegg --lib <managed-process/shell focused filters>
cargo test -p egggit
cargo test -p codegg-git
cargo test -p codegg-core worktree --lib
cargo test -p codegg --lib <git focused filters>
cargo test -p codegg --lib command_intent
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_tool_broker_boundary.py
```

The implementing agent should use current command names/arguments from the repository if any of these have changed.

Hosted `CI / verify` may be used for exact-head evidence where local host architecture/toolchain limitations recur. Do not add a new workflow or lane.

## 11. Static guards

Do not add a new static guard unless M009 fixes a demonstrated ownership bypass that cannot be protected by an existing guard. Prefer correcting the existing ownership manifest/guard to reflect the canonical architecture.

A guard change must never merely suppress an inconvenient valid finding.

## 12. Storage, protocol, migration, and compatibility effects

Expected default: none.

M009 should not require schema, protocol, durable identity, or user-visible compatibility changes. If triage reveals that a real project-catalog defect requires a storage migration or that a tool-boundary defect requires a protocol change, stop and register a separately bounded corrective plan.

Historical run/session/projection data must remain readable. Existing bounded compatibility transports remain unchanged unless current evidence identifies a security/correctness defect.

## 13. Security and failure requirements

Any code change in M009 must preserve fail-closed behavior:

- no authority widening to bypass a failing guard;
- no raw secret persistence;
- no shell substitution for typed argv merely to make a test pass;
- no path inference from process cwd where typed workspace context is required;
- no direct tool execution that skips required authorization/admission/audit;
- no weakening Git environment/redaction policy;
- no mutation outside checked edit/history boundaries.

If a verification failure is environmental, document the environment and obtain current compatible-host evidence rather than weakening the check.

## 14. Acceptance criteria

M009 is complete when all of the following are true:

- the current M001-M004/M006 condition matrix is complete and evidence-backed;
- every runnable historical condition has current direct or equivalent evidence;
- the M004 shell timeout has a current explicit disposition;
- all three M008 guard findings have current explicit owner/disposition;
- any true architecture-convergence-owned defect found is fixed narrowly with focused coverage;
- any material out-of-scope defect has its own registered corrective plan instead of being hidden in the closure record;
- no new CI/verification machinery was introduced;
- normal existing formatting/Clippy/quick verification passes to the extent supported by the final candidate, with any remaining environmental condition named precisely;
- M009 closure record gives a recommendation of `closed`, `conditionally closed`, `corrective pass required`, or `blocked` using the planning vocabulary;
- registry reflects that recommendation truthfully.

## 15. Stop conditions

Stop and create a separately registered corrective plan rather than continuing M009 if any of the following are required:

- schema/storage migration;
- public protocol change;
- redesign of scheduler/tool broker/process service/AgentLoop/context/Git/LSP/projection boundaries;
- broad project-catalog rewrite;
- new CI infrastructure;
- cross-platform support expansion;
- more than a small local production correction to the shell timeout or ReviewTool path.

A conditional final disposition is acceptable and preferred to scope expansion.

## 16. Required closure evidence

`plans/closure/architecture-convergence-incomplete-verticals/009-status.md` must contain:

- final implementation/evidence commit SHAs;
- before/after aggregate status and explanation of why the prior `closed` state was inconsistent;
- per-milestone M001/M002/M003/M004/M006 condition matrix;
- exact tests/guards run and their outcomes;
- M004 shell-timeout analysis and disposition;
- daemon-cwd, project-catalog, and ReviewTool guard disposition matrix;
- all M009 code changes, if any, mapped to demonstrated defects;
- migration/protocol/storage compatibility statement;
- security/authority review;
- unresolved findings by severity and owner;
- final recommendation for the subsystem registry status;
- any newly registered follow-up plan, with explicit reason M009 did not absorb it.
