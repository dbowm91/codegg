# CodeGG Active Planning Registry

This file is the compact control surface for active interim planning. Detailed requirements and completed history remain in source roadmaps, implementation plans, `plans/closure/`, and Git history.

Canonical direction remains in:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

## Status vocabulary

- **proposed** — roadmap or plan exists but is not approved for execution.
- **ready** — dependencies and interfaces are satisfied; plan may be handed off.
- **active** — implementation or closure work is in progress.
- **blocked** — a named dependency or evidence requirement prevents progress.
- **closing** — implementation landed and closure evidence is being gathered.
- **closed** — closure record accepted.
- **conditionally closed** — substantial work landed, but a named correctness or operational evidence condition remains.
- **superseded** — replaced by another document.
- **archived** — no longer active and retained for traceability.

## Active subsystem roadmaps

| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Architecture convergence and incomplete verticals | ready | `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md` | M001/M002/M003/M008 ready | M004 waits on M001-M003; M005 waits on M003; M006 waits on M004; M007 waits on M002's stable execution/edit boundary. |
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 closed | — |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 4 closed | — |
| Provider connections and Eggpool | closed | `plans/subsystems/provider-direct-call-session-context-corrective-addendum.md` | M009 closed | Direct production provider callers receive owning session/run context; M008 transport/header behavior remains preserved. |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 closed | — |
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestones 001-004 closed | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | Milestone 012 closed | — |
| Agent runtime, model adaptation, and ACP | closed | `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md` | M017 closed | — |
| Agent runtime correctness, autonomy, and simplification | closed | `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md` | M011 closed | Exact candidate `e3b671ad`; hosted run `31525206176` / job `93891703941` passed through Workspace tests. |
| Agent runtime — goal verification corrective follow-up | closed | `plans/subsystems/agent-runtime-goal-verification-corrective-addendum.md` | M013 closed | Exact-goal provenance, conservative criteria, and cross-goal evidence isolation accepted. |
| Agent runs, async delegation, and worktree concurrency | closed | `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md` | M009 closed | Root completion, invocation scope, group-terminal projection, and exact-head CI corrections accepted. |
| Agent convergence and independent verification | closed | `plans/subsystems/agent-convergence-roadmap.md` | M003 closed | Bounded repair/replan, explicit commit chaining, conservative model gating, and projection closure accepted. |
| Memory-to-skill promotion | closed | `plans/subsystems/memory-skill-promotion-hosted-verification-corrective-addendum.md` | M005 closed | Publication/proposal Clippy findings and exact-head hosted closure accepted. |
| Runtime consolidation, deletion, and footprint | closed | `plans/subsystems/runtime-consolidation-deletion-footprint-tui-closure-addendum.md` | M010 closed | Durable TUI schedule identity and labels reconciled. |
| Programmatic tool execution and Tool Programs | closed | `plans/subsystems/tool-programs-roadmap.md` | M019 strict closure + M020 corrective disposition accepted | — |
| Development verification and release | closed | `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md` | M008/M009 closed; M007 remains closed | Minimal verification posture retained. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Only the previously recorded supported-Linux Landlock fixture evidence remains. |
| Runtime safety — checked edit-history corrective follow-up | closed | `plans/subsystems/runtime-safety-edit-history-corrective-addendum.md` | M013 closed | Exact candidate `f314c38e` passed hosted `CI / verify`. |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-daemon-lifecycle-corrective-addendum.md` | C003 closed | Daemon startup/shutdown/process lifecycle corrective work accepted. |
| Search and eggsearch integration | closed | `plans/subsystems/search-eggsearch-integration-roadmap.md` | M005 closed | — |

## Dependency-ready implementation plans

| Subsystem | Milestone | Plan | Why ready |
|---|---|---|---|
| Architecture convergence and incomplete verticals | M001 — context and compaction ownership convergence | `plans/implementation/architecture-convergence-incomplete-verticals/001-context-compaction-ownership-convergence.md` | Prior runtime/context/provider corrective dependencies are closed; this milestone can inventory and converge current owners independently. |
| Architecture convergence and incomplete verticals | M002 — process and tool execution ownership convergence | `plans/implementation/architecture-convergence-incomplete-verticals/002-process-tool-execution-ownership-convergence.md` | Tool Programs, runtime safety, and daemon/process lifecycle dependencies are closed; no new scheduler/tool runtime is required. |
| Architecture convergence and incomplete verticals | M003 — Git ownership convergence | `plans/implementation/architecture-convergence-incomplete-verticals/003-git-ownership-convergence.md` | Agent-run/worktree and edit-history dependencies are closed; ownership can be converged without changing durable Git/worktree semantics. |
| Architecture convergence and incomplete verticals | M008 — headless projection consumer and legacy transport disposition | `plans/implementation/architecture-convergence-incomplete-verticals/008-headless-projection-consumer-legacy-transport-disposition.md` | Session projections are already closed; a second consumer can validate the existing contract independently of M001-M007. |

## Architecture convergence execution order

1. M001, M002, M003, and M008 are dependency-ready. M001-M003 may run in parallel if implementation agents coordinate root wiring edits; M008 is independent.
2. M004 begins only after M001-M003 close. It must consume the converged context, process/tool, and Git owners rather than introduce temporary extraction layers.
3. M005 begins after M003 closes so rerun/replay uses one stable Git/worktree/provenance boundary. It may proceed in parallel with M004 once M003 is closed.
4. M006 begins after M004 closes so command-pipeline simplification targets the final AgentLoop coordinator boundary instead of churning an unstable interface.
5. M007 begins when M002 closes or exposes its stable execution/edit integration contract; checked edit-history and LSP preview ownership remain otherwise unchanged.
6. The roadmap does not register new packaging/release automation, Windows support expansion, another scheduler/tool/plugin runtime, another memory subsystem, or another verification framework.
7. Verification for every milestone remains focused tests plus the existing `scripts/verify.sh quick` posture and existing hosted CI only where strict closure requires exact-head evidence.

## Blocked work

| Subsystem | Milestone | Blocker |
|---|---|---|
| Architecture convergence and incomplete verticals | M004 — AgentLoop coordinator reduction | Hard-blocked on M001, M002, and M003 closure. |
| Architecture convergence and incomplete verticals | M005 — durable run rerun/replay completion | Hard-blocked on M003 Git ownership convergence. |
| Architecture convergence and incomplete verticals | M006 — command pipeline convergence | Hard-blocked on M004 AgentLoop coordinator reduction. |
| Architecture convergence and incomplete verticals | M007 — controlled LSP mutation application | Interface-blocked until M002 closes or provides a stable canonical process/edit boundary. |
| Runtime safety, resource control, and footprint | C002 supported-Linux evidence | Historical supported-Linux Landlock fixture evidence remains outstanding; it does not block the new architecture-convergence roadmap. |

No other newly registered plan is hard-blocked.

## Closure work and recently completed control points

Detailed historical milestone history is intentionally not duplicated here; use source subsystem roadmaps, corrective addenda, `plans/closure/`, and Git history. Current control points relevant to new work are:

| Subsystem | Milestone | Status | Closure / controlling evidence |
|---|---|---|---|
| Agent runs, async delegation, and worktree concurrency | M009 — root completion delivery, invocation scope, exact-head closure | closed | `plans/closure/agent-run-worktree-concurrency/009-status.md` |
| Agent runtime — goal verification | M013 — goal evidence provenance and criterion corrective pass | closed | `plans/closure/agent-runtime-correctness-autonomy-simplification/013-status.md` |
| Agent convergence and independent verification | M003 — bounded repair/replan and model gating | closed | `plans/closure/agent-convergence/003-status.md` |
| Runtime consolidation, deletion, and footprint | M010 — TUI durable schedule identity and label closure | closed | `plans/closure/runtime-consolidation-deletion-footprint/010-status.md` |
| Runtime safety — checked edit-history corrective follow-up | M013 — cross-session checkpoint atomicity and hosted closure | closed | `plans/closure/runtime-safety-resource-footprint/013-status.md` |
| Provider connections and Eggpool | M009 — direct provider session-context corrective pass | closed | `plans/closure/provider-connections/009-status.md` |
| Programmatic tool execution and Tool Programs | M019/M020 — strict closure and child-artifact recovery | closed | `plans/closure/tool-programs/019-status.md`; `plans/closure/tool-programs/020-status.md` |
| Search and eggsearch integration | M005 — hosted closure and SourceCard fidelity | closed | `plans/closure/search-eggsearch-integration/005-status.md` |
| Development verification and release | M007-M009 — minimal verification and hosted corrective closures | closed | `plans/closure/development-verification-release/007-status.md`; `008-status.md`; `009-status.md` |
| Memory-to-skill promotion | M005 — publication Clippy and hosted closure | closed | `plans/closure/memory-skill-promotion/005-status.md` |

Historical closure records MUST NOT be rewritten to conceal predecessor defects or failed verification. Corrective work, if discovered during the new roadmap, must receive a new milestone/addendum under the normal planning process.

## Verification policy

Verification remains deliberately light. New architecture-convergence milestones may add focused unit/integration tests or a narrow static guard where it enforces a real ownership invariant, but they MUST NOT add:

- new CI lanes;
- new security scanners;
- coverage, benchmark, or binary-size gates;
- dependency bots;
- workflow-dispatch/release automation;
- a fixed release cadence.

The normal broad local posture remains:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Hosted `CI / verify` is closure evidence on exact candidates when the existing closure convention requires it; it is not a new architecture requirement.

## Deferred unregistered product work

These remain outside active handoff unless a concrete product priority or new evidence makes them ready:

- packaging/distribution improvements such as crates.io or prebuilt binaries;
- expanding Windows from opportunistic compatibility to a guaranteed support tier;
- full web/desktop/mobile frontends;
- arbitrary LSP `workspace/executeCommand` support;
- binary topology split or separate daemon/TUI packaging without measured deployment need;
- replacing RustPython with a custom Tool Program parser;
- generalized HTTP/provider-client unification;
- broad Comrak/MSRV or Ratatui dependency migrations;
- final team roles, presence, and chat;
- production hosted Tool Program transport;
- seccomp, namespace, container, or remote-execution sandbox expansion;
- persistent search indexing;
- automatic dependency-update bots or continuous binary-size/audit gates;
- release automation or a fixed release cadence.
