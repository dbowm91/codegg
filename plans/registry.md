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
| Architecture convergence and incomplete verticals | conditionally closed | `plans/subsystems/architecture-convergence-strict-closure-corrective-addendum.md` | M009 conditionally closed | Compatible-host root runtime and strict all-feature Clippy evidence remains outstanding. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Supported-Linux Landlock fixture evidence remains outstanding. |
| Residual runtime consolidation | closed | `plans/subsystems/residual-runtime-consolidation-roadmap.md` | M001–M003 closed | — |
| Identity, authorization, and audit | closed | `plans/subsystems/identity-authorization-audit-roadmap.md` | M005 closed | — |
| Presence and read-only observation | closed | `plans/subsystems/presence-observation-roadmap.md` | M001–M003 closed | — |
| Project collaboration | closed | `plans/subsystems/project-collaboration-roadmap.md` | M001+M002+M003 closed | — |
| Interactive process sessions | closed | `plans/subsystems/interactive-process-sessions-roadmap.md` | M001–M003 closed | — |
| Tool Program capability expansion | active | `plans/subsystems/tool-program-capability-expansion-roadmap.md` | M002 ready | M003 depends on the M002 seam; M001 eligibility contract closed. |
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 closed | — |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 4 closed | — |
| Provider connections and Eggpool | closed | `plans/subsystems/provider-direct-call-session-context-corrective-addendum.md` | M009 closed | — |
| Provider authentication capability closure | closed | `plans/subsystems/provider-auth-capability-closure-addendum.md` | M010 closed | — |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 closed | — |
| Multi-project TUI and sessions | closed | `plans/subsystems/tui-project-sessions-roadmap.md` | Milestones 001-004 closed | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | M012 closed | — |
| Agent runtime, model adaptation, and ACP | closed | `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md` | M017 closed | — |
| Agent runtime correctness, autonomy, and simplification | closed | `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md` | M011/M013 follow-up closed | — |
| Agent runs, async delegation, and worktree concurrency | closed | `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md` | M009 closed | — |
| Agent convergence and independent verification | closed | `plans/subsystems/agent-convergence-roadmap.md` | M003 closed | — |
| Memory-to-skill promotion | closed | `plans/subsystems/memory-skill-promotion-hosted-verification-corrective-addendum.md` | M005 closed | — |
| Runtime consolidation, deletion, and footprint | closed | `plans/subsystems/runtime-consolidation-deletion-footprint-tui-closure-addendum.md` | M010 closed | — |
| Programmatic tool execution and Tool Programs | closed | `plans/subsystems/tool-programs-roadmap.md` | M019/M020 closed | Expansion is separately registered above; historical roadmap remains closed. |
| Development verification and release | closed | `plans/subsystems/development-verification-release-ci-reproducibility-corrective-addendum.md` | M010 closed | Operator action remains requiring `CI / verify` on `main`. |
| Distribution and installation | closed | `plans/subsystems/distribution-installation-roadmap.md` | M001/M002 closed | — |
| Runtime safety — checked edit-history corrective follow-up | closed | `plans/subsystems/runtime-safety-edit-history-corrective-addendum.md` | M013 closed | — |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-daemon-lifecycle-corrective-addendum.md` | C003 closed | — |
| Post-audit maintainability and surface | closed | `plans/subsystems/post-audit-maintainability-surface-roadmap.md` | M001-M005 closed | — |
| Search and eggsearch integration | closed | `plans/subsystems/search-eggsearch-integration-roadmap.md` | M005 closed | — |

## Dependency-ready implementation plans

| Subsystem | Milestone | Plan | Why ready |
|---|---|---|---|
| Tool Program capability expansion | M002 — external repo search | `plans/implementation/tool-program-capability-expansion/002-external-search-programmatic-read-seam.md` | M001 eligibility contract closed; ADR-0001 supplies the authority contract. |

These four plans may be implemented in parallel if agents avoid overlapping documentation/module edits. A plan moves to `active` only when implementation actually begins.

## Current execution order and dependency gates

1. Residual M001–M003 are closed and the subsystem roadmap is closed (stranded team/shell-session surfaces retired, closure at `plans/closure/residual-runtime-consolidation/001-status.md`; CoreDaemon request-family decomposition, closure at `plans/closure/residual-runtime-consolidation/002-status.md`; CoreDaemon construction/lifecycle decomposition, closure at `plans/closure/residual-runtime-consolidation/003-status.md`).
2. Identity/auth/audit is closed: M001 domain -> M002 transport identity -> M003 daemon authorization/attribution (closed) -> M004 audit store (closed) -> M005 instrumentation closure (closed).
3. Presence M001/M002/M003 are closed and the subsystem is complete. Observation consumes the already-closed session-projection interface.
4. Project collaboration M001+M002+M003 are closed and the subsystem is complete: durable authorized channels/messages with ordering/retry/restart/retention/auth/privacy, bounded TUI chat with observer insert routing and zero-control proof, and separately authorized structured actions with idempotent scheduler-boundary submits and audit causation. Free text is never an execution interface.
5. Interactive-process M001/M002/M003 are closed; the subsystem roadmap is closed.
6. Tool Program expansion M001 is closed (eligibility contract + `diff` promotion, closure at `plans/closure/tool-program-capability-expansion/001-status.md`). M002 external search may proceed on that contract. M003 narrow Git/LSP reads remain blocked behind M002; no mutation authority is added.
7. None of this work authorizes a new scheduler, service bus, verification framework, release automation, persistent search index, remote sandbox, or broader Windows support tier.

## Blocked work

| Subsystem | Milestone | Blocker |
|---|---|---|
| Architecture convergence | M009 strict operational evidence | Compatible-host root runtime / all-feature Clippy evidence. |
| Runtime safety | C002 supported-Linux evidence | Historical Landlock supported-Linux fixture evidence. |
| Tool Program capability expansion | M003 operation-scoped Git/LSP reads | TP expansion M002 closure. |

## Closure work and current control points

Detailed historical milestone history is intentionally not duplicated here. Current foundation evidence relevant to the new batch includes:

| Subsystem | Status | Controlling evidence |
|---|---|---|
| Identity, authorization, and audit | M001 closed | `plans/closure/identity-authorization-audit/001-status.md` |
| Identity, authorization, and audit | M002 closed | `plans/closure/identity-authorization-audit/002-status.md` |
| Identity, authorization, and audit | M003 closed | `plans/closure/identity-authorization-audit/003-status.md` |
| Identity, authorization, and audit | M004 closed | `plans/closure/identity-authorization-audit/004-status.md` |
| Identity, authorization, and audit | M005 closed | `plans/closure/identity-authorization-audit/005-status.md` |
| Post-audit maintainability and surface | closed | `plans/closure/post-audit-maintainability-surface/001-status.md` through `005-status.md` |
| Provider authentication capability closure | closed | `plans/closure/provider-connections/010-status.md` |
| Frontend-neutral session projections | closed | `plans/closure/session-projections/012-status.md` |
| Agent runs/worktree concurrency | closed | `plans/closure/agent-run-worktree-concurrency/009-status.md` |
| Tool Programs | closed | `plans/closure/tool-programs/019-status.md`; `plans/closure/tool-programs/020-status.md` |
| Tool Program capability expansion | M001 closed | `plans/closure/tool-program-capability-expansion/001-status.md` |
| Residual runtime consolidation | M001 closed | `plans/closure/residual-runtime-consolidation/001-status.md` |
| Residual runtime consolidation | M002 closed | `plans/closure/residual-runtime-consolidation/002-status.md` |
| Residual runtime consolidation | M003 closed | `plans/closure/residual-runtime-consolidation/003-status.md` |
| Search and eggsearch integration | closed | `plans/closure/search-eggsearch-integration/005-status.md` |
| Presence and read-only observation | M001 closed | `plans/closure/presence-observation/001-status.md` |
| Presence and read-only observation | M002 closed | `plans/closure/presence-observation/002-status.md` |
| Presence and read-only observation | M003 closed | `plans/closure/presence-observation/003-status.md` |
| Project collaboration | M001 closed | `plans/closure/project-collaboration/001-status.md` |
| Project collaboration | M002 closed | `plans/closure/project-collaboration/002-status.md` |
| Project collaboration | M003 closed | `plans/closure/project-collaboration/003-status.md` |

Historical closure records MUST NOT be rewritten to conceal predecessor defects or failed verification. New defects receive new corrective plans under the owning subsystem.

## Verification policy

Verification remains deliberately light. Newly registered milestones may add focused unit/integration tests or a narrow static guard where it enforces a real ownership invariant, but they MUST NOT add new CI lanes, scanners, coverage/benchmark/binary-size gates, dependency bots, release automation, or fixed release cadence.

Normal broad local posture remains:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Hosted `CI / verify` is closure evidence only where the existing closure convention or an exact operational condition requires it.

## Deferred unregistered product work

These remain outside active handoff unless concrete product priority/evidence makes them dependency-ready:

- distribution expansion beyond the closed Linux/macOS binary+installer slice, including Homebrew/deb/rpm/Nix, Windows installer support, signing/notarization, SBOM/provenance and package-manager automation;
- expanding Windows from opportunistic compatibility to a guaranteed support tier;
- full web/desktop/mobile frontends;
- arbitrary LSP `workspace/executeCommand` support;
- binary topology split or separate daemon/TUI packaging without measured deployment need;
- replacing RustPython with a custom Tool Program parser;
- generalized HTTP/provider-client unification;
- broad Comrak/MSRV or Ratatui dependency migrations;
- production hosted Tool Program transport;
- seccomp, namespace, container, or remote-execution sandbox expansion;
- persistent search indexing;
- automatic dependency-update bots or continuous binary-size/audit gates;
- OAuth device/PKCE or external-command provider auth until a concrete provider/security contract justifies activation;
- remote workspace/node/distributed execution phases until identity/audit dependencies make them ready;
- release automation or a fixed release cadence.
