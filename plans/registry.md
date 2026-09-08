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
| Architecture convergence and incomplete verticals | conditionally closed | `plans/subsystems/architecture-convergence-strict-closure-corrective-addendum.md` | M009 conditionally closed | Guard findings resolved; compatible-host root runtime and strict all-feature Clippy evidence remains outstanding. |
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 closed | — |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 4 closed | — |
| Provider connections and Eggpool | closed | `plans/subsystems/provider-direct-call-session-context-corrective-addendum.md` | M009 closed | Direct production provider callers receive owning session/run context; M008 transport/header behavior remains preserved. |
| Provider authentication capability closure | active | `plans/subsystems/provider-auth-capability-closure-addendum.md` | M010 ready | Existing auth/provider ownership is stable; stored bearer support is bounded to compatible credential-capable provider paths. |
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
| Development verification and release | closed | `plans/subsystems/development-verification-release-ci-reproducibility-corrective-addendum.md` | M010 closed | Exact-head hosted `CI / verify` green on `8fc7b3b5`; closure at `plans/closure/development-verification-release/010-status.md`. Operator action: require `CI / verify` on `main`. |
| Distribution and installation | active | `plans/subsystems/distribution-installation-roadmap.md` | M001 ready | Manual release authority and single-binary topology are stable; M002 waits on the artifact/checksum contract. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Only the previously recorded supported-Linux Landlock fixture evidence remains. |
| Runtime safety — checked edit-history corrective follow-up | closed | `plans/subsystems/runtime-safety-edit-history-corrective-addendum.md` | M013 closed | Exact candidate `f314c38e` passed hosted `CI / verify`. |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-daemon-lifecycle-corrective-addendum.md` | C003 closed | Daemon startup/shutdown/process lifecycle corrective work accepted. |
| Post-audit maintainability and surface | active | `plans/subsystems/post-audit-maintainability-surface-roadmap.md` | M001, M003, M004 ready | M002 hard-blocked on M001; M005 hard-blocked on M002 with an interface dependency on M003. |
| Search and eggsearch integration | closed | `plans/subsystems/search-eggsearch-integration-roadmap.md` | M005 closed | — |

## Dependency-ready implementation plans

| Subsystem | Milestone | Plan | Why ready |
|---|---|---|---|
| Post-audit maintainability and surface | M001 — compatibility-surface rationalization | `plans/implementation/post-audit-maintainability-surface/001-compatibility-surface-rationalization.md` | Canonical subsystem owners already exist; the work is an evidence-backed compatibility census/disposition. |
| Post-audit maintainability and surface | M003 — agent-runtime physical decomposition | `plans/implementation/post-audit-maintainability-surface/003-agent-runtime-physical-decomposition.md` | Remaining problem is physical concentration inside canonical agent ownership; M001 is only a soft merge dependency. |
| Post-audit maintainability and surface | M004 — Bash-tool physical decomposition | `plans/implementation/post-audit-maintainability-surface/004-bash-tool-physical-decomposition.md` | Bash execution/sandbox/scheduler owners are already stable; this is a behavior-preserving locality refactor. |
| Provider authentication capability closure | M010 — auth capability matrix and stored bearer closure | `plans/implementation/provider-connections/010-auth-capability-matrix-and-stored-bearer-closure.md` | Credential kinds/store/factory seams already exist; the missing support matrix and stored-bearer mismatch are concrete. |
| Distribution and installation | M001 — manual prebuilt artifact contract | `plans/implementation/distribution-installation/001-manual-prebuilt-artifact-contract.md` | Manual release governance and single-binary topology are closed; stable artifact/checksum packaging can be implemented independently of release automation. |

## Architecture convergence execution order

1. M001-M004 remain historically conditionally closed; M005 is closed; M006 remains historically conditionally closed; M007 and M008 are closed. Their predecessor closure records are immutable evidence and must not be rewritten.
2. M009 is conditionally closed by `plans/closure/architecture-convergence-incomplete-verticals/009-status.md`; its compatible-host root runtime evidence condition remains explicit and is not a new architecture pass.
3. M009 must reproduce/disposition the historical M004 shell timeout and triage the three M008 guard findings: daemon cwd usage, project-catalog invariant drift, and direct ReviewTool execution.
4. Small directly owned defects may be corrected inside M009. Any migration, protocol change, broad subsystem redesign, or new verification infrastructure requires a separate registered corrective plan rather than scope expansion.
5. The architecture-convergence subsystem returns to `closed` only if M009 establishes that no architecture-convergence condition remains; otherwise it must remain `conditionally closed` with the remaining condition named explicitly.
6. The workstream still does not register release automation, Windows support expansion, another scheduler/tool/plugin runtime, another memory subsystem, or another verification framework. The separately registered distribution roadmap is manual and does not change this rule.
7. Verification remains focused tests plus the existing `scripts/verify.sh quick` posture and existing hosted CI only where current exact-head closure evidence genuinely requires it.

## Post-audit execution order

1. CI M010, maintainability M001/M003/M004, provider-auth M010, and distribution M001 are independent dependency-ready handoffs. Agents implementing them in parallel must avoid overlapping edits rather than inventing cross-plan dependencies.
2. Maintainability M002 begins only after M001 establishes canonical names and compatibility disposition.
3. Maintainability M005 begins only after M002 closes and after M003 has stabilized the agent/runtime construction seams it consumes.
4. Distribution M002 begins only after M001 fixes the release asset names, archive layout, checksum manifest, and completeness rules.
5. The active distribution work does not authorize tag-triggered, scheduled, workflow-dispatch, or routine-CI release automation. Cadence/version/publication remain manual maintainer decisions.
6. Provider-auth M010 must leave OAuth device flow and external-command credential execution explicitly unsupported; it is not a general auth expansion.
7. Team roles/presence/chat, additional frontends, persistent indexing, new sandboxes, and Windows support-tier expansion remain outside this batch.

## Blocked work

| Subsystem | Milestone | Blocker |
|---|---|---|
| Runtime safety, resource control, and footprint | C002 supported-Linux evidence | Historical supported-Linux Landlock fixture evidence remains outstanding; it is independent of architecture-convergence M009. |
| Post-audit maintainability and surface | M002 — model-visible tool-surface minimization | Hard dependency on M001 canonical-name/compatibility disposition. |
| Post-audit maintainability and surface | M005 — runtime-service context and mutable-global cleanup | Hard dependency on M002 final tool construction/disclosure contract; interface dependency on M003 agent construction seams. |
| Distribution and installation | M002 — verified installer and end-user installation | Hard dependency on M001 stable target/asset/archive/checksum contract. |

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
| Development verification and release | M007-M010 — minimal verification and hosted corrective closures | closed | `plans/closure/development-verification-release/007-status.md`; `008-status.md`; `009-status.md`; `010-status.md` |
| Memory-to-skill promotion | M005 — publication Clippy and hosted closure | closed | `plans/closure/memory-skill-promotion/005-status.md` |
| Architecture convergence and incomplete verticals | M001 — context and compaction ownership convergence | conditionally closed | `plans/closure/architecture-convergence-incomplete-verticals/001-status.md` |
| Architecture convergence and incomplete verticals | M002 — process and tool execution ownership convergence | conditionally closed | `plans/closure/architecture-convergence-incomplete-verticals/002-status.md` |
| Architecture convergence and incomplete verticals | M003 — Git ownership convergence | conditionally closed | `plans/closure/architecture-convergence-incomplete-verticals/003-status.md` |
| Architecture convergence and incomplete verticals | M004 — AgentLoop coordinator reduction | conditionally closed | `plans/closure/architecture-convergence-incomplete-verticals/004-status.md` |
| Architecture convergence and incomplete verticals | M005 — durable run rerun/replay completion | closed | `plans/closure/architecture-convergence-incomplete-verticals/005-status.md` |
| Architecture convergence and incomplete verticals | M006 — command pipeline convergence | conditionally closed | `plans/closure/architecture-convergence-incomplete-verticals/006-status.md` |
| Architecture convergence and incomplete verticals | M007 — controlled LSP mutation application | closed | `plans/closure/architecture-convergence-incomplete-verticals/007-status.md` |
| Architecture convergence and incomplete verticals | M008 — headless projection consumer and legacy transport disposition | closed | `plans/closure/architecture-convergence-incomplete-verticals/008-status.md` |
| Architecture convergence and incomplete verticals | M009 — strict closure evidence and guard triage | conditionally closed | `plans/closure/architecture-convergence-incomplete-verticals/009-status.md` |

Historical closure records MUST NOT be rewritten to conceal predecessor defects or failed verification. Corrective work, if discovered during the new roadmaps, must receive a new milestone/addendum under the normal planning process.

## Verification policy

Verification remains deliberately light. Newly registered milestones may add focused unit/integration tests or a narrow static guard where it enforces a real ownership invariant, but they MUST NOT add:

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

Hosted `CI / verify` is closure evidence on exact candidates when the existing closure convention requires it; it is not a new architecture requirement. Development-verification M010 specifically requires a green exact-head run because a current hosted/local reproducibility contradiction is its defect trigger.

## Deferred unregistered product work

These remain outside active handoff unless a concrete product priority or new evidence makes them ready:

- distribution expansion beyond the active Linux/macOS binary+installer slice, including Homebrew/deb/rpm/Nix, Windows installer support, signing/notarization, SBOM/provenance, and package-manager automation;
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
- OAuth device/PKCE or external-command provider auth until a concrete provider/security contract justifies activation;
- release automation or a fixed release cadence.
