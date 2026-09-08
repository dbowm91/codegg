# Post-Audit Maintainability and Surface Milestone 004 — Bash-Tool Physical Decomposition

Status: ready for handoff

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Source roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md` where Bash participates in controlled programmatic execution.

Primary class: polish / invariant

## 1. Objective

Decompose the approximately 125 KiB `src/tool/bash.rs` into a small model-facing `BashTool` facade plus coherent internal modules for command policy/classification, supervised process execution/cancellation, and bounded output/result handling, without creating a second shell executor or changing any sandbox, scheduler, permission, child-Git, persistence, or resource-control semantics.

## 2. Why this milestone is ready

Bash is already the canonical model-facing shell tool and previous convergence/runtime-safety work has established its surrounding owners: permission classification, sandbox/path policy, command-intent routing, scheduler submission, process supervision, child-Git restrictions, run persistence, output projection, and bounded output.

The audit finding is physical concentration, not an unresolved ownership decision. M004 can therefore proceed independently of M001–M003 as long as shared imports/registration code are kept stable.

Recent baseline work also added an explicit Git output cap and converted Python blocking filesystem work, reinforcing the repository-wide expectation that process/output resource semantics are correctness contracts rather than incidental implementation details.

## 3. Current implementation evidence

`src/tool/bash.rs` is approximately 125,192 bytes and contains several concerns that should be mapped before moving code. The implementer must identify at minimum:

- `BashTool` configuration/builders and `Tool` implementation;
- command parsing/token inspection and destructive/safe classification;
- workspace/path and sensitive-path validation;
- child-Git command restrictions;
- command-intent classification/routing integration;
- scheduler-owned versus directly supervised execution decisions;
- process spawn environment, cwd, stdin/stdout/stderr setup;
- timeout/cancellation/process-tree termination;
- bounded stdout/stderr capture and truncation;
- shell output projection and artifact/run-store persistence;
- error/status/result shaping;
- test fixtures and tests for the above.

Existing neighboring modules such as `tool::destructive`, command-intent/shell/projector/runtime helpers, scheduler services, and sandbox modules must be treated as existing owners. Do not copy their logic into newly extracted Bash modules.

## 4. Invariants that must not regress

- `BashTool` remains the single model-facing `bash` tool.
- Permission and destructive-command checks happen before an unauthorized command can spawn.
- Workspace/sensitive-path policy remains fail-closed according to current contract.
- Child-agent/child-Git policy remains narrower than parent authority and cannot be bypassed by alternate shell spelling.
- Heavy/durable work continues to use scheduler-owned submission where current command-intent policy requires it.
- Directly supervised local commands retain bounded timeout/cancellation and process-tree cleanup.
- No unbounded stdout/stderr accumulation is introduced.
- Run/artifact persistence retains session/run/workspace attribution and bounded content behavior.
- Shell projection/redaction remains on the accepted output path.
- Environment/working-directory construction remains explicit and uses typed workspace context where provided.
- No second command parser becomes an independent security authority if one accepted classifier already exists.

## 5. Scope

### In scope

- Build a responsibility map of `bash.rs` and neighboring owners.
- Move Bash-owned policy/classification glue into a coherent internal module where it is not already owned elsewhere.
- Move direct process supervision into a coherent internal module.
- Move output capture/result/persistence glue into a coherent internal module.
- Keep `BashTool` builders/config and top-level execution sequencing readable.
- Move focused tests alongside responsibilities where practical.
- Tighten private/module visibility where safe.
- Remove duplicate helper logic discovered during extraction when one canonical neighboring owner already exists.

### Explicitly out of scope

- Replacing the shell, adding a shell parser dependency, or implementing a full shell AST.
- Rewriting scheduler or sandbox policy.
- Adding new command-intent families.
- Changing which commands are considered destructive except to fix a concrete defect found by existing tests/evidence; such a change must be documented separately in closure.
- New remote execution backends.
- PTY/interactive-shell feature expansion.
- Changing user permission UX.
- File-size CI gates.

## 6. Required production changes

### Core/domain

The expected shape is approximately:

```text
src/tool/bash.rs                # BashTool configuration + high-level execute sequence
src/tool/bash/policy.rs         # Bash-owned classification/policy glue only
src/tool/bash/process.rs        # direct supervised spawn/wait/cancel/kill mechanics
src/tool/bash/output.rs         # bounded capture/projection/persistence/result shaping
```

The exact layout may differ. Reuse existing `destructive`, scheduler, shell/projector, sandbox, and command-intent modules rather than creating duplicate `policy.rs` implementations for behavior they already own.

The high-level sequence should remain visibly auditable:

```text
parse/classify
 -> authorize/path/child policy
 -> route to scheduler or local supervised process
 -> collect bounded result
 -> project/redact/persist
 -> return structured tool result
```

### Storage and migrations

No migration expected. Run-store/artifact records must keep the same IDs, fields, truncation/content-handle semantics, and transactional behavior.

### Protocol and DTOs

No protocol change expected. Structured tool result/provenance shapes remain compatible.

### Runtime and concurrency

The process module must make child ownership explicit:

- stdin policy;
- stdout/stderr pipe ownership;
- maximum capture size;
- timeout/cancellation select behavior;
- kill/process-group semantics;
- wait/reap semantics;
- behavior if output readers finish before/after child exit;
- behavior if persistence/projection fails after the process completed.

Do not introduce detached tasks merely to make module boundaries convenient. Any reader/supervisor task must have an explicit join/cancellation owner.

### Frontend or operator surface

No user-visible behavior change expected. Diagnostics should retain command classification/routing/result information already exposed.

### Security and authorization

Security-sensitive checks must remain in a deterministic pre-spawn chain. Extraction should make this order easier to audit.

A source-level regression test/guard is warranted only if an extraction could easily permit `spawn` before policy checks; otherwise focused negative tests are preferable.

### Documentation and static guards

Update `architecture/tool.md`, shell/execution/sandbox documentation, and `AGENTS.md` path notes if they currently point to `bash.rs` as the sole implementation location. Do not add a generic line-count check.

## 7. Ordered work packages

### Work package A — Map the Bash execution pipeline

Intent: establish existing owners and pre-spawn ordering.

Required actions:

1. Inventory functions/types/tests in `bash.rs`.
2. Mark each as facade/config, policy/classification, process supervision, output/persistence, or external-owner glue.
3. Draw the pre-spawn authorization sequence and the post-spawn cancellation/output sequence.
4. Identify duplicated helpers whose canonical owner already exists outside `bash.rs`.

Acceptance evidence:

- concise ownership/pipeline map;
- explicit list of security checks that must precede spawn.

### Work package B — Extract pure policy/classification glue

Intent: separate decision-making from execution.

Required changes:

- move Bash-owned pure classification/routing helper functions;
- invoke existing destructive/path/command-intent owners rather than copying them;
- move focused positive/negative policy tests.

Acceptance evidence:

- policy tests run without spawning processes where feasible;
- prohibited commands remain rejected before process construction.

### Work package C — Extract supervised process execution

Intent: isolate process lifetime/resource mechanics.

Required changes:

- encapsulate child construction, pipes, timeout/cancellation, kill, wait/reap, and bounded reader coordination;
- preserve environment/cwd/sandbox hooks and scheduler boundary;
- return a typed internal result rather than already formatted model text.

Acceptance evidence:

- timeout/cancel/exit-code/output-cap tests;
- no orphan/leaked child in negative paths;
- no unbounded read-to-end without cap.

### Work package D — Extract output/result/persistence handling

Intent: keep process mechanics separate from model projection and durable metadata.

Required changes:

- transform typed process/scheduler results through existing output projector/redactor;
- persist run/artifact metadata through existing store APIs;
- preserve truncation/content handles and error precedence.

Acceptance evidence:

- output truncation/projection/persistence tests;
- persistence failure does not misreport a command as never executed.

### Work package E — Reduce facade and reconcile docs

Intent: leave `BashTool::execute` auditable as high-level sequencing.

Required changes:

- remove moved helpers/tests from root file;
- inspect module imports for cycles/pass-through wrappers;
- update architecture/source-layout docs.

Acceptance evidence:

- facade clearly shows the policy → execution → result sequence;
- descriptive before/after file sizes and ownership notes in closure.

## 8. Failure, cancellation, restart, and contention semantics

Policy failure: no process/scheduler job is started; error is typed/actionable according to current behavior.

Spawn failure: no output/persistence path may imply successful execution.

Timeout/cancellation: terminate the owned process tree/group according to existing platform semantics, drain/join bounded readers as required, reap the child, then report terminal status. Do not leave a detached reader/process.

Output overflow: cap/truncate according to existing contract rather than OOM or blocking forever.

Persistence/projection failure after command completion: preserve the true command terminal outcome internally and return/report the secondary failure without pretending the process did not run. Follow current precedence if already specified.

Restart: in-flight local direct shell processes are not made magically resumable by this refactor. Durable scheduler/run-store behavior remains as currently owned.

Concurrent Bash calls must continue to respect scheduler/global resource admission and independent process/output state.

## 9. Compatibility and migration

The user-facing tool name/input schema/output contract remain unchanged unless a concrete existing bug is separately documented. Model profiles and permissions continue referring to `bash`.

No config or persisted-data migration expected. Internal module paths are not a compatibility target unless documented as public downstream API.

## 10. Required tests

### Focused unit tests

- command classification/destructive/path/child-Git decisions;
- output truncation/result shaping;
- builder/default configuration semantics.

### Integration tests

- successful direct command;
- nonzero exit;
- scheduler-routed command where existing tests support it;
- cwd/workspace handling;
- run-store/artifact persistence;
- output projector/redaction.

### Restart and recovery tests

No new direct-process restart behavior. Run existing scheduler/durable-run recovery tests if shared result code is touched.

### Contention and cancellation tests

- timeout kills and reaps child;
- explicit cancellation kills/reaps child tree;
- bounded stdout/stderr readers terminate;
- simultaneous calls do not share mutable output/process state.

### Security and negative tests

- destructive/unauthorized command cannot reach spawn;
- child-Git restriction cannot be bypassed;
- sensitive/out-of-root path checks retain fail-closed semantics;
- environment/argument output redaction remains intact where applicable.

### Migration and compatibility tests

No new migration test expected; retain existing tool-schema/golden behavior tests.

## 11. Required verification commands

Discover exact selectors at implementation head. Expected focused coverage:

```bash
cargo test -p codegg tool::bash
cargo test -p codegg tool::destructive
cargo test -p codegg shell::
cargo test -p codegg scheduler::

python3 scripts/check_execution_ownership.py
python3 scripts/check_sandbox_contract.py

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Run only relevant scheduler/shell subsets first; do not create another broad verification lane for this refactor.

## 12. Documentation updates

- `architecture/tool.md` Bash ownership/layout.
- Shell/process execution and sandbox architecture docs that name implementation files.
- `AGENTS.md` source-layout notes.
- No README/user documentation changes unless behavior genuinely changes.

## 13. Acceptance criteria

- Bash responsibilities are mapped before extraction.
- `BashTool` remains the single model-facing owner and its high-level sequencing is auditable.
- Policy/classification, process supervision, and output/result handling have coherent independently testable homes.
- All security checks that previously preceded spawn still precede spawn.
- Timeout/cancellation/process cleanup and output caps remain correct.
- Scheduler and run-store ownership remains unchanged.
- No duplicate shell executor, sandbox policy, or command router is introduced.
- Broad repository verification remains minimal and green according to commands actually run.

## 14. Stop conditions

Stop and report when:

- extraction would require changing scheduler/sandbox/permission ownership;
- safe decomposition appears to require a new shell parser/runtime dependency;
- borrow/lifetime issues appear solvable only by detached tasks or shared mutable globals;
- tests reveal a substantive command-policy defect whose correction is larger than this refactor;
- public tool input/output behavior would need a breaking change;
- another active plan materially changes the same Bash pipeline before implementation can rebase safely.

## 15. Closure evidence required

- implementation commits/PRs;
- before/after ownership/pipeline map and descriptive file sizes;
- list of final Bash modules/responsibilities;
- pre-spawn security-order evidence;
- focused policy/process/output tests and outcomes;
- timeout/cancel/process-reap/output-cap evidence;
- execution/sandbox guard outcomes;
- formatting/lint/quick verification results;
- compatibility statement and any incidental bug fixes explicitly called out;
- remaining concentration and rationale.

## 16. Handoff notes

Do not use module extraction as an excuse to duplicate logic currently owned by `destructive`, sandbox, scheduler, shell projector, or command-intent services. The best result is a smaller Bash facade with clearer calls into those owners.

Preserve platform-specific process cleanup behavior. If Unix and Windows paths differ inside the current file, keep that distinction explicit rather than forcing a misleading common abstraction.
