# Architecture Convergence Strict Closure Corrective Addendum

Status: active — M009 dependency-ready

Repository baseline reviewed: `1b0f8d076c71b5783769d9dae2b4efd5afa1d047`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Historical closure records retained unchanged:

- `plans/closure/architecture-convergence-incomplete-verticals/001-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/002-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/003-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/004-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/005-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/006-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/007-status.md`
- `plans/closure/architecture-convergence-incomplete-verticals/008-status.md`

Corrective implementation plan:

- `plans/implementation/architecture-convergence-incomplete-verticals/009-strict-closure-evidence-and-guard-triage.md`

Planning references:

- `plans/003-planning-process.md#2.5-closure-records`
- `plans/003-planning-process.md#4-dependency-model`
- `plans/003-planning-process.md#5-milestone-sizing`
- existing architecture-convergence roadmap invariants and non-goals

Primary class: corrective verification / closure / maintenance triage

## 1. Purpose

Reconcile the architecture-convergence workstream with CodeGG's planning vocabulary after M008 marked the source roadmap closed while M001-M004 and M006 still retain explicit conditional-closure evidence requirements.

M009 is intentionally not a feature milestone. It must establish the strongest truthful final disposition available for the prior work, resolve small owned verification defects where justified, and triage three pre-existing guard findings surfaced during M008 without broadening into unrelated subsystem redesign.

The architecture established by M001-M008 remains the accepted production direction. M009 must not reopen those ownership boundaries unless current repository evidence demonstrates an actual correctness defect.

## 2. Corrective evidence requiring action

At baseline `1b0f8d0`, the registry and M008 closure simultaneously assert:

- the architecture-convergence subsystem is `closed`;
- M001, M002, M003, M004, and M006 are `conditionally closed`;
- their remaining conditions are primarily host-toolchain/runtime evidence rather than known production defects.

This is internally inconsistent with the registry's own status vocabulary. `closed` means closure evidence has been accepted without a named outstanding condition; `conditionally closed` means a named correctness or operational evidence condition remains.

The predecessor closure records must remain unchanged. M009 owns later evidence and the final aggregate disposition.

## 3. Existing conditional-closure inventory

M009 must begin by re-reading the current closure records and reducing their remaining conditions to an exact matrix. At baseline the known items are:

| Milestone | Existing condition | Initial M009 disposition |
|---|---|---|
| M001 | focused compaction runtime test could not execute on the mixed-architecture local host | seek exact-head/current-head execution evidence on a compatible host; do not change compaction architecture merely to satisfy the test |
| M002 | focused managed-process/runtime tests and strict feature-heavy Clippy were incomplete on the local host | rerun only the named existing commands needed to resolve the condition |
| M003 | root focused Git runtime test and strict all-features Clippy were blocked by local host issues / unrelated lint | rerun current relevant evidence; do not absorb unrelated subsystem work automatically |
| M004 | local host-toolchain evidence incomplete; hosted workspace tests reproduced one existing shell runtime timeout test failure while M004-scoped tests passed | determine whether the shell timeout is still reproducible on current head and either repair the small owning defect or record it as independently owned follow-up |
| M006 | local focused command-pipeline runtime test could not link and the closure was written before hosted workspace testing fully completed | obtain current exact-head/current-head evidence sufficient to accept or retain the operational condition |

M009 must not infer that old environmental failures still exist if current hosted/current-head evidence resolves them. Conversely, it must not erase a condition merely because later unrelated work passed some broader command.

## 4. Guard findings to triage

M008 recorded three low-severity pre-existing findings:

1. two Python-script `current_dir` findings from `check_daemon_cwd_usage.py`;
2. project-catalog storage-layout expectation drift from `check_project_catalog_invariants.py`;
3. a direct ReviewTool execution site reported by `check_tool_broker_boundary.py`.

M009 must classify each finding as exactly one of:

- false positive / guard expectation drift;
- already fixed on current head;
- small architecture-convergence-owned defect suitable for correction in M009;
- valid defect owned by another existing subsystem;
- valid defect requiring a separately registered follow-up because fixing it would materially exceed M009 scope.

The direct ReviewTool execution finding deserves explicit ownership analysis because it may intersect the execution/tool-boundary convergence established by M002. Classification must be evidence-based; M009 must not force all tool execution through a new abstraction if the direct path is an intentional typed authority boundary.

## 5. Invariants

M009 must preserve all M001-M008 invariants, including:

- daemon execution and durable-state authority;
- scheduler admission/resource authority;
- frontend-neutral projections with no TUI-owned durable state;
- typed workspace/project/session/run identity;
- child authority never wider than parent authority;
- one canonical CodeGG compaction policy owner;
- one canonical finite-process lifecycle owner with explicit justified protocol exceptions;
- explicit Git ownership among `egggit`, `codegg-git`, core, and root adapters;
- AgentLoop as coordinator rather than duplicate policy owner;
- rerun as a fresh child run without persisted raw secrets;
- one canonical command planning/dispatch flow;
- checked mutation authority for LSP edits;
- bounded, non-authoritative legacy transports.

Historical closure records are immutable evidence and MUST NOT be rewritten to conceal predecessor limitations.

## 6. Explicit non-goals

M009 MUST NOT:

- add a new scheduler, tool registry, plugin runtime, memory subsystem, projection protocol, or workflow engine;
- redesign AgentLoop, compaction, Git, command routing, LSP mutation, rerun, or projection architecture absent a demonstrated correctness defect;
- add new CI lanes, scanners, benchmark gates, coverage gates, binary-size gates, dependency bots, or release automation;
- make Windows support, packaging/distribution, or full frontend work part of closure;
- fix every pre-existing repository guard failure merely because it is visible;
- require all historical conditional milestones to become unconditionally closed if a named external/toolchain condition is genuinely still not reproducible or resolvable within this bounded pass;
- weaken or delete tests/guards simply to obtain green verification.

## 7. Dependency graph

```text
M001-M008 historical implementation and closure evidence
                    |
                    v
M009 strict closure evidence + bounded guard triage
```

M009 has no hard external dependency beyond current `main`. It is dependency-ready immediately.

The unrelated runtime-safety C002 supported-Linux Landlock fixture condition remains independent and must not be absorbed into this workstream.

## 8. Milestone M009 — Strict closure evidence and guard triage

Status: ready

Plan:

- `plans/implementation/architecture-convergence-incomplete-verticals/009-strict-closure-evidence-and-guard-triage.md`

Objective:

Produce a truthful final architecture-convergence disposition by resolving the outstanding M001-M004/M006 operational evidence where current infrastructure permits, correcting only small directly-owned defects exposed by that evidence, and assigning the three M008 guard findings to explicit owners.

Exit conditions:

- a current requirement/evidence matrix exists for every conditional M001-M004/M006 item;
- all named evidence that can run on the repository's normal supported environment is rerun without inventing new verification infrastructure;
- the M004 shell timeout test is either green on current head, fixed with a small owning correction, or registered as a separate bounded follow-up with evidence that it is outside M009;
- each M008 guard finding has an evidence-backed owner/disposition;
- any M009 code change is minimal and directly tied to a demonstrated defect;
- `cargo fmt --all -- --check`, the existing workspace Clippy command where applicable, and `scripts/verify.sh quick` are used rather than new gates;
- hosted `CI / verify` is used only if needed for exact-head closure evidence under existing conventions;
- `plans/closure/architecture-convergence-incomplete-verticals/009-status.md` records predecessor conditions, commands actually run, results, guard dispositions, unresolved findings, and final recommendation;
- registry status becomes `closed` only if no architecture-convergence condition remains; otherwise it becomes `conditionally closed` with the remaining condition named explicitly.

## 9. Final disposition rule

The subsystem may return to `closed` only when M009's closure record demonstrates that the prior conditional evidence has either:

1. been satisfied by current compatible-host/current-head evidence;
2. been superseded by later exact evidence that directly exercises the same invariant; or
3. been converted into a separately owned, independently registered defect whose existence no longer makes the architecture-convergence closure claim inaccurate.

If any condition still directly qualifies the architecture-convergence implementation itself, the subsystem remains `conditionally closed`. Truthful conditional closure is preferred over expanding verification machinery or manufacturing unrelated corrective work.

## 10. Deferred work

The following remain deferred unless M009 produces new concrete evidence requiring a separate plan:

- packaging/prebuilt binaries and release distribution;
- Windows support-tier expansion;
- arbitrary LSP `workspace/executeCommand`;
- removal of bounded external `/ws` compatibility without migration evidence;
- broad project-catalog redesign;
- broad tool-broker redesign;
- general daemon cwd/path cleanup outside demonstrated production ownership violations;
- runtime-safety C002 Landlock fixture evidence.
