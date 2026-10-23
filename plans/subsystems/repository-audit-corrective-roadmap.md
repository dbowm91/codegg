# Repository Audit Corrective Roadmap

Status: proposed

Repository baseline reviewed: `d5be7acd05dc999888430b7ad23420d554359fec` (`origin/main`)
Audit evidence was gathered at `04536eeeb`; every file this workstream cites is
byte-identical at that commit and at the baseline above, so the findings carry
forward unchanged.

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `architecture/core.md`
- `architecture/tui.md`
- `architecture/tool.md`
- `architecture/authorization.md`
- `architecture/git.md`
- `architecture/overview.md`

Related ADRs:

- **None required for the work as scoped.** No milestone in this roadmap changes a canonical identity, an ownership boundary, a protocol, a storage layout, or the authentication/authorization model.
- **accepted ADR-0045 (`plans/adrs/ADR-0045-command-syntax-and-execution-authority.md`) is governing for C002 and is not reopened.** ADR-0045 makes command syntax **advisory**: classifiers may annotate risk and escalate to the ApprovalRouter, but "their failure to recognize an action cannot expand the sandbox or bypass explicit operator/tool authorization", and built-in syntax-only terminal blockers "should be reclassified as risk/approval signals after a documented compatibility pass" — explicitly **not** replaced by "an equally bypassable command parser elsewhere". ADR-0028 is a **historical alias** archived by the identity migration; the shell-policy line under it is closed (`plans/subsystems/shell-command-policy-adr0028-post-closure-corrective-addendum.md`).
- A **new ADR is required** if C002's implementation would make an advisory risk signal terminal, or if C004 would redefine the domain's terminality contract. Both are explicit stop conditions in their plans.

## 1. Purpose and ownership boundary

This workstream carries the findings of a full-repository audit executed at baseline `04536eeeb`. The audit was read-only, produced no code change, and covered the core runtime, the scheduler, the TUI, the tool and plugin layers, the LSP and provider crates, and the in-code comment corpus.

It owns:

- the removal of process-terminating panics reachable from user-facing commands;
- the coverage of the destructive-command detector for the command spellings that actually execute;
- the argument contract between the command registry and the command handlers;
- the consistency of job terminality across the scheduler's notice path and the goal-evidence path;
- the re-enablement of static guards that exist but are not wired, and the platform asymmetry of the lint gate;
- the removal of permanently uncompiled code;
- the per-frame cost of the TUI composer, the streaming transcript, and the tasks overlay;
- the reduction of the in-code comment corpus and the prevention of its re-inflation.

It consumes, and does not redefine:

- daemon and scheduler execution authority;
- the `ToolBroker` production tool-call boundary;
- the command registry and the TUI key-ownership model;
- the provider, ACP, projection, plugin, Git, and Tool Program contracts;
- the single-binary, single-daemon topology;
- the closed shell-policy and interactive-process-authorization lines.

The governing rule is:

> Fix the defects that can kill a process, destroy a user's filesystem, or silently drop a user's input first. Then restore the verification surface that should have caught them. Only then spend effort on cost and legibility, and spend it only where the evidence is measured.

## 2. Work classification

### Invariants

- No user-facing command may terminate the process by panicking. A command that cannot complete must fail closed with a message, not unwind.
- The advisory risk classification of a command is **truthful about the spellings that execute**: a catastrophic root delete is reported as catastrophic whatever the flag order or trailing options, and a legitimate scoped delete is not reported as catastrophic. The signal stays advisory, per accepted ADR-0045; authority is unchanged.
- The argument a command's handler receives is the argument the user typed, and no command is reachable with an argument that still contains its own name.
- A job state has exactly one terminality classification, and every projection of that state — the scheduler's notices, the goal-evidence path, the model-facing status string — agrees with it.
- A guard that documents a contract is executed by the verification command it is named in.
- The lint gate is exercised on the platforms the project ships for, or the asymmetry is stated and owned.
- Code that cannot be compiled cannot be relied upon and is not retained as history.
- The comment corpus records the current contract. It does not record what used to be true, and it does not duplicate the owning `architecture/` document.

### Capabilities

- A user who runs an `/lsp-*` command receives a result or a refusal; the TUI does not exit.
- A user who runs a destructive command receives a refusal for the spellings that would destroy their filesystem, and an unambiguous result for the ones that would not.
- A user who types `/search foo` searches for `foo`.
- A user or operator reading a job's status sees the job's real state, not a flattened `"running"`.

### Infrastructure

- The broker-boundary guard is executable, non-vacuous, and wired into both `scripts/verify.sh quick` and CI.
- The lint gate runs on macOS and Linux with the same command and the same expectations.
- The comment corpus has a measured budget and a guard that fails on drift.

### Polish

- The composer, the streaming transcript, the tasks overlay, and the file tree stop re-deriving the same result more than once per frame.
- The in-code comment corpus is smaller, its stale references are repaired, and the planning registry's oversized rows are reduced to pointers.

**Infrastructure and polish are not presented as completed capability.** The performance and comment milestones change no user-visible behaviour and are sequenced after the correctness milestones close, because a polish phase created before the capability's correctness boundary is closed is a prohibited planning anti-pattern.

## 3. Non-goals

- No new model, provider, tool, or protocol.
- No change to the scheduler's admission keys, the writer-permit model, the `ToolBroker` boundary, or the daemon's ownership of durable state.
- No reopening of any closed milestone, closure record, or ADR. Where a closed line recorded a residual finding, this roadmap cites it and takes only what it explicitly left open.
- No change to what the shell policy denies. C002 repairs the **truthfulness of the advisory risk signal**; accepted ADR-0045 still decides what follows a classification.
- No re-architecture of the TUI's command registry, the `App` struct, or the render loop. The 161-arm dispatcher and the 20k-line `App` module are **recorded** here as the structural cause of C003's defect class, and are explicitly **not** in scope to restructure.
- No rewriting of `plans/closure/` records or `docs/validation/` records. Those are historical evidence whose line citations are pinned by design.
- No performance claim without a measurement. Allocation counts derived from data structures are acceptable as mechanism; wall-clock numbers are not, unless measured.

## 4. Current state

All findings below were reproduced against the baseline by reading the code and, where stated, by executing the repository's own tooling.

**A process-terminating panic is reachable from the shipped TUI and CLI.** `tokio::runtime::Handle::block_on` panics with *"Cannot start a runtime from within a runtime"* when called on a thread whose runtime context is already entered. The TUI's event loop is awaited inside `#[tokio::main(flavor = "multi_thread")]`, so the context is entered for the whole loop. Seventeen `/lsp-*` slash commands, the `/worktree` command, and the `codegg mcp debug` subcommand each call `handle.block_on(..)` from that context. `App::on_key` is synchronous and is not covered by the event loop's only `catch_unwind`, which wraps rendering. The unit tests do not reproduce the defect because they enter the runtime with `Runtime::enter()`, which sets the current handle but **not** the entered flag, so the nested `block_on` succeeds there. The correct pattern — `block_in_place` — is already used at three other sites in the same file.

**The destructive-command risk signal misreports in both directions.** ADR-0045 makes command syntax advisory, so a classifier's job is to *tell the truth about the risk* and escalate to the ApprovalRouter — not to guarantee safety. It currently fails to do that in two ways. The catastrophic-delete classifier splits on the literal prefix `rm -rf /` and the regex form requires the `/` to follow the flags immediately and to end the string, so both effective spellings of a root delete — `rm -rf --no-preserve-root /` and `rm -rf / --no-preserve-root` — are reported as *not risky*, while the form `rm -rf /` that GNU coreutils *refuses* is reported as catastrophic. In the other direction, the human `!cmd` path's built-in block pattern ends with a bare `/` and no anchor, so it hard-blocks the legitimate, scoped `rm -rf /tmp/build`, which is exactly the obstruction ADR-0045 says creates bypass pressure.

**Six registered slash commands never receive their arguments.** The command palette's `query` holds the entire typed line, including the command name. Six handlers read it without stripping the name, so `/search foo` searches for the literal `"/search foo"` and `/task-del 5` yields the id `"/task-del 5"`. The dispatcher contains 161 arms and two competing argument idioms — 101 consume the `raw_input` parameter and 73 re-parse the palette query. This is the same dispatcher that carries the nested-`block_on` defect, and the same growth that makes both defects invisible: each arm looks locally correct.

**Job terminality has two answers.** The scheduler's `is_final_job_state` deliberately excludes `Interrupted`, on the documented grounds that an eligible attempt may still be requeued and must never be announced as final. The goal-evidence assembler maps `Interrupted` to a terminal `Failed`, and a unit test pins that behaviour. A sibling map projects `Scheduled`, `Queued` and `Blocked` as `"running"`, so the model-facing job status cannot distinguish a queued job from a running one.

**A guard that enforces a stated invariant is not executed.** `scripts/check_tool_broker_boundary.py` exits 1 on the baseline, is absent from `scripts/verify.sh`'s guard set and from CI, and is recorded in closure records as known-red. Of its six findings, one is a genuine boundary question — `src/permission/reviewer.rs` calls `ToolRegistry::execute_capture` directly, from outside the broker — and five are false positives caused by a pattern that matches intra-tool `self.` delegation. The guard's own remedy text names `self.tool_broker.execute()`.

**The lint gate is platform-asymmetric.** `cargo clippy --workspace --all-targets --locked -- -D warnings` fails on macOS at the baseline. Every failing site is production code that exists only on Linux, so the Linux CI runner does not compile it and CI is unaffected. A macOS developer running `scripts/verify.sh full` therefore sees failures that CI will never reproduce.

**Permanently uncompiled code is retained.** `src/tui/app/mod.rs` carries 615 lines of `#[cfg(any())]`-disabled function body with no callers, whose own doc comment records that the convergence that supersedes it is complete.

**The TUI's per-frame cost is dominated by content, not by the loop.** The event loop is well built: a dirty flag, a render interval of zero or 16 ms, saturating timing arithmetic, no busy-wait, one frame source, and a fetch of LSP status that is kept off the render path. The costs are elsewhere: the streaming tail is re-parsed as Markdown in full on every frame; the composer re-wraps its entire buffer four times per frame with no cache and no size cap, and clones the whole buffer once per frame and again per keystroke; the editor's syntax-highlight cache is keyed on the document revision, which every local keystroke bumps, so the whole cache is discarded and re-scanned per keystroke; the tasks overlay builds an unbounded row list, deep-clones it, and truncates to twelve rows afterwards; the primary transcript is never trimmed while the sibling chat surface caps at a hundred messages per channel.

**The comment corpus is large, and the rationale it repeats already exists.** Roughly 112,000 comment lines sit at a 0.145 comment-to-code ratio. The defensible reduction is about 7–8% of the corpus — roughly 8,800 to 10,000 lines — and the honest answer is that no larger number is defensible. What makes the reduction safe is that the rationale is already canonical in `architecture/`: the code comments are a third copy of it. Twenty-three path references in comments are stale and resolve to nothing. The planning registry, which every agent is directed to read first, is about 99,900 words, with 88% of it inside table cells and about thirty rows carrying 500 to 2,000 words each where the median row carries 44.

## 5. Target architecture

No new architecture. The end state is that the existing contracts are met by the code that ships:

```text
command keypress
  -> App::on_key (synchronous, no panic path)
     -> command registry (one argument contract: name stripped once, at the entry point)
        -> handler receives the argument the user typed
           -> async work is spawned, never blocked on

scheduler job state
  -> one terminality classification (JobState)
     -> scheduler notices      (terminal iff the domain says terminal)
     -> goal-evidence assembly (terminal iff the domain says terminal)
     -> model-facing status    (distinct projection per state, never flattened)

destructive command
  -> classification (advisory; recognises the spellings that execute)
     -> accepted ADR-0045 decides what follows

verification
  -> every guard named in verify.sh quick actually runs
  -> lint gate runs on every shipped platform with the same expectation
```

## 6. Dependency graph

```text
C001 nested-runtime panic removal
C002 destructive-delete classification coverage
C003 slash-command argument contract
C004 job-state terminality and status projection
C005 verification surface and dead code
C006 TUI render-path and transcript cost
C007 comment corpus reduction and drift guard
```

| Milestone | Depends on | Class of dependency |
|---|---|---|
| C001 | — | — |
| C002 | — | — |
| C003 | — | — |
| C004 | — | — |
| C005 | C001, C002, C003, C004 **closed** | hard (it re-enables the gate that should have caught them) |
| C006 | C001 and C003 closed | hard (both touch the TUI command and render path this milestone measures) |
| C007 | C001–C006 closed | hard (the corpus budget must be initialised from final measurements) |

C001–C004 have no hard or interface dependency on each other and may be executed in parallel on separate branches. C005's dependency is **soft in execution but hard in ordering of the gate**: the broker-boundary guard must be re-enabled only after the genuine finding it reports is dispositioned, or the re-enablement would immediately turn the verification surface red. C006 and C007 are polish and are deliberately last.

## 7. Milestones

### Milestone C001 — Nested-runtime panic removal in the TUI and MCP CLI

Class: invariant

Objective: no user-facing command can terminate the process by panicking on a nested runtime entry.

Dependencies: none.

Deliverable boundary: the 21 identified call sites, the command-dispatch path they sit behind, and a regression test that enters the runtime the way production does.

User or operator value: the TUI and the MCP debug subcommand stop being one keystroke away from exiting.

Exit conditions: no unguarded `Handle::block_on` remains on a production path reachable from a runtime context; a regression test that reproduces the production entry path fails on the baseline and passes after the change; the panic is not merely relocated to a different site.

Deferred work: the structural cause — a 161-arm dispatcher in a 20k-line module — is recorded in the roadmap's §4 and is not restructured here.

### Milestone C002 — Destructive-delete classification coverage

Class: invariant

Objective: the advisory risk classification of a destructive command is truthful in both directions — it reports the spellings that actually execute as catastrophic, and it stops reporting a legitimate scoped delete as catastrophic.

Dependencies: none.

Deliverable boundary: the risk classifier in the tool path, the built-in block pattern in the human-shell path, and the fixtures that prove the classification.

User or operator value: the approval path sees the risk it should see, and the legitimate scoped delete is not obstructed — which is what removes the pressure to route around the check.

Exit conditions: every effective spelling of a catastrophic delete is reported as catastrophic; the scoped spellings that were previously refused are not; the signal remains advisory and no authority boundary moves; a user-configured deny is preserved untouched.

Deferred work: the policy itself is ADR-0045's and is untouched. The broader ADR-0045 migration of the remaining built-in syntax-only terminal blockers is not this milestone's.

### Milestone C003 — Slash-command argument contract

Class: capability

Objective: a registered command's handler receives the argument the user typed, with the command name removed.

Dependencies: none.

Deliverable boundary: the command registry's entry point, the dispatcher's argument handling, and the affected handlers.

User or operator value: `/search foo` searches for `foo`; `/task-del 5` deletes task `5`.

Exit conditions: every registered command with an argument receives it; a single argument-extraction point exists; the six known-defective commands are covered by tests that fail on the baseline; the `Dialog::is_open` predicate covers every dialog variant.

Deferred work: splitting the 161-arm dispatcher is not in scope.

### Milestone C004 — Job-state terminality and status projection

Class: capability

Objective: one terminality classification, honoured by every projection of a job's state.

Dependencies: none.

Deliverable boundary: the scheduler's terminality predicate, the goal-evidence assemblers, and the model-facing status projection.

User or operator value: a job that is queued, blocked, or interrupted is reported as such rather than as running or failed.

Exit conditions: the domain's terminality classification is the single source; every projection agrees with it; the model-facing status distinguishes the non-terminal states; the divergence is closed by a decision recorded in the closure, not by editing a test expectation.

Deferred work: the dead routing abstraction in the agent router is recorded but is not removed here.

### Milestone C005 — Verification surface and dead code

Class: infrastructure

Objective: every guard the repository names is executed, the lint gate is exercised on every shipped platform, and code that can never compile is removed.

Dependencies: C001–C004 closed.

Deliverable boundary: the broker-boundary guard and its wiring, the lint gate's platform coverage, and the 615-line disabled function body.

User or operator value: a defect of the C001–C004 class is caught by the verification command rather than by a user.

Exit conditions: the broker guard is green on the real tree, non-vacuous by injection, and wired into both `verify.sh quick` and CI; the macOS and Linux lint expectations are stated and the platform-specific findings are either fixed or explicitly owned; the disabled code is gone and the module compiles without it.

Deferred work: the `research/` crates' exclusion from the workspace lint policy is recorded as a residual finding, not fixed here.

### Milestone C006 — TUI render-path and transcript cost

Class: polish

Objective: the composer, the streaming transcript, the tasks overlay, and the file tree stop re-deriving the same result more than once per frame.

Dependencies: C001 and C003 closed.

Deliverable boundary: the composer's wrap and layout path, the streaming tail's parse path, the tasks overlay's row construction, the file tree's row cache, the editor's highlight cache key, and the transcript's retention.

User or operator value: typing and streaming stay responsive as content grows, and a long session does not retain a second rendered copy of the whole transcript forever.

Exit conditions: each identified re-derivation is either cached against a stable key or removed; the caches are invalidated by the edit that actually invalidates them; the transcript has a stated retention policy consistent with the sibling chat surface's; a measurement is recorded for each change, and where a measurement is unavailable the closure says so.

Deferred work: a benchmark harness is not created; without one, no wall-clock claim is made.

### Milestone C007 — Comment corpus reduction and drift guard

Class: polish

Objective: the in-code comment corpus is smaller, its stale references are repaired, and it cannot re-inflate.

Dependencies: C001–C006 closed.

Deliverable boundary: the stale path references, the fix-archaeology comments, the plan-id restatements, the oversized registry rows, and a new guard with a measured budget.

User or operator value: a reader — human or agent — reaches the current contract with less text, and a stale pointer fails the build instead of misleading.

Exit conditions: the stale references resolve or are corrected; the reduction is measured and stated honestly, including the ceiling; the invariant that a comment states a rule the code cannot enforce is preserved, and the pointer that replaces a removed essay survives; the guard is non-vacuous, wired into `verify.sh quick` and CI, and does not report "the tree is clean" when it could not read the tree.

Deferred work: the two comment-sensitive guards are hardened in a separate recorded change, not as part of the cleanup.

## 8. Cross-cutting requirements

### Storage and migration

No milestone changes a storage layout, a durable row, or a schema version. C004 changes a projection, not a stored value.

### Protocol and compatibility

No milestone changes a wire protocol or a DTO. C004's model-facing status change is a **projection**: the distinct non-terminal states become distinguishable to the model, which is additive for a consumer that already received `"running"` and strictly more truthful.

### Security and authorization

C002 is the security-relevant milestone. It repairs the **truthfulness of an advisory risk signal** inside accepted ADR-0045's contract, and removes a built-in over-block that obstructs a legitimate scoped operation. It must not widen or narrow authority, must not turn the signal terminal, and must not substitute a new bypassable parser for the old one. The closure must state the before-and-after classification for a fixture set that includes both the effective dangerous spellings and the scoped ones previously refused.

C001 is a safety milestone in the sense that a panic is an uncontrolled termination: the TUI's state is not flushed and the user's session is left as it was on disk.

### Concurrency, cancellation, and recovery

C001 replaces a blocking wait with a spawned task. The spawn must not change the command's completion semantics: the result still reaches the surface that asked for it, and a command that cannot complete still fails closed with a message.

### Observability and audit

A panic that kills the TUI is currently unobservable after the fact. C001's change must leave a diagnostic where one is cheap, and must not add a second log sink.

### Performance and resource use

C006's acceptance is measured, not asserted. Where a measurement cannot be produced in the environment, the closure records that fact and claims only the mechanism.

### Documentation and operations

C007 reduces documentation. It must not remove a rule that exists nowhere else, and it must not rewrite `plans/closure/` or `docs/validation/` records. Any guard that pins a line number must be updated in the same change, and the drift of a pinned citation in a historical record is **not** a defect to fix.

## 9. Verification strategy

- **C001** — a regression test that drives a command through the production entry path, with the runtime entered the way `#[tokio::main]` enters it. A test that enters with `Runtime::enter()` is not evidence and must be rejected.
- **C002** — a fixture set of destructive and benign command spellings, asserted against the classifier's verdict rather than against a pattern.
- **C003** — one test per affected command, asserting the argument the handler receives.
- **C004** — a property over the job-state enum asserting that every projection agrees with the domain's terminality classification.
- **C005** — the guards run against the real tree; non-vacuity is proved by injection, as the repository's existing guards do it.
- **C006** — before/after measurement of the specific re-derivation, with the environment recorded.
- **C007** — the guard's own `--self-test`, plus an injection that makes it fail on the real tree.

## 10. Risks and decision points

| # | Risk or decision | Owner |
|---|---|---|
| D1 | C002's repair could become a policy change. If making the risk signal truthful would require making it **terminal**, or would require a new syntax parser, **stop** — accepted ADR-0045 forbids both. | ADR-0045 |
| D2 | C004's terminality authority is **already settled** — `JobState::is_terminal()` exists in the domain crate and excludes `Interrupted` — so no ADR is required and the milestone aligns projections with an existing contract. The stop condition remains: if the implementer finds that reconciling a projection would **redefine** the domain's terminality rather than align with it, **stop** and record an ADR. | none — discharged by the domain's existing contract |
| D3 | C001's spawn changes the command's completion semantics. If the surface cannot accept an asynchronous result without a design change, **stop**. | C001 plan |
| D4 | C005's lint-gate work may surface a macOS-only defect that is not macOS-specific in nature. Fix it; do not silence it with a blanket allow. | C005 plan |
| D5 | C007's reduction may remove a rule that exists nowhere else. The keep-list in the plan §"Must not delete" is the constraint; a removal that cannot be traced to an owning `architecture/` section is not permitted. | C007 plan |
| D6 | The interactive-process authorization finding — all ten `InteractiveProcess*` requests are `ScopeKind::Global` with no capability, and a remote principal can create a PTY and run commands but not terminate it — is **already registered** as `remote-interactive-process-authorization-corrective` R001, status `proposed`, new ADR required. It is **not** duplicated here. | that line |

## 11. Completion definition

The roadmap is closed when:

- C001–C005 are closed, each with a closure record that names its evidence and its unresolved findings;
- C006 and C007 are closed, or are explicitly recorded as deferred with a named owner;
- the audit's findings are each either closed here, or recorded as owned by another line, with no finding left unowned;
- the registry's rows for this line are consistent with the roadmap and the closure records;
- no milestone closed on compilation alone.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | not started | `plans/implementation/repository-audit-corrective/001-nested-runtime-panic-removal.md` | — | — |
| C002 | **closed** | `plans/implementation/repository-audit-corrective/002-destructive-delete-classification-coverage.md` | `plans/closure/repository-audit-corrective/002-status.md` | — |
| C003 | not started | `plans/implementation/repository-audit-corrective/003-slash-command-argument-contract.md` | — | — |
| C004 | not started | `plans/implementation/repository-audit-corrective/004-job-state-terminality-and-status-projection.md` | — | — |
| C005 | not started | `plans/implementation/repository-audit-corrective/005-verification-surface-and-dead-code.md` | — | C001–C004 closed |
| C006 | not started | `plans/implementation/repository-audit-corrective/006-tui-render-path-and-transcript-cost.md` | — | C001, C003 closed |
| C007 | not started | `plans/implementation/repository-audit-corrective/007-comment-corpus-reduction-and-drift-guard.md` | — | C001–C006 closed |
