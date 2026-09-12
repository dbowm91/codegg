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
| Upstream tool-surface compatibility corrective | closing | `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md` | M009 closing | Implementation and verification landed; closure record is being finalized. |
| Repository surface housekeeping corrective | closed | `plans/subsystems/repository-surface-housekeeping-corrective-addendum.md` | M001 closed | — |
| Post-audit maintainability and surface — corrective | closed | `plans/subsystems/post-audit-maintainability-surface-corrective-addendum.md` | M006/M007 closed | Search/eggsearch configured fallback remains intentionally closed/retained. |
| Architecture convergence and incomplete verticals | conditionally closed | `plans/subsystems/architecture-convergence-strict-closure-corrective-addendum.md` | M009 conditionally closed | Compatible-host root runtime and strict all-feature Clippy evidence remains outstanding. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Supported-Linux Landlock fixture evidence remains outstanding. |
| Post-implementation maintainability closure | closed | `plans/subsystems/post-implementation-maintainability-closure-roadmap.md` | M001 closed | — |
| Residual runtime consolidation | closed | `plans/subsystems/residual-runtime-consolidation-roadmap.md` | M001-M003 closed | — |
| Identity, authorization, and audit | closed | `plans/subsystems/identity-authorization-audit-roadmap.md` | M005 closed | — |
| Presence and read-only observation | closed | `plans/subsystems/presence-observation-roadmap.md` | M001-M003 closed | — |
| Project collaboration | closed | `plans/subsystems/project-collaboration-roadmap.md` | M001+M002+M003 closed | — |
| Interactive process sessions | closed | `plans/subsystems/interactive-process-sessions-roadmap.md` | M001-M003 closed | — |
| Tool Program capability expansion | closed | `plans/subsystems/tool-program-capability-expansion-roadmap.md` | M001-M003 closed | — |
| Domain identity and compatibility | closed | `plans/subsystems/domain-identity-roadmap.md` | Milestone 4 closed | — |
| Runtime assets and harness interoperability | closed | `plans/subsystems/runtime-assets-roadmap.md` | Milestone 4 closed | — |
| Provider connections and Eggpool | closed | `plans/subsystems/provider-direct-call-session-context-corrective-addendum.md` | M009 closed | — |
| Provider authentication capability closure | closed | `plans/subsystems/provider-auth-capability-closure-addendum.md` | M010 closed | — |
| Project catalog and lazy discovery | closed | `plans/subsystems/project-catalog-roadmap.md` | Milestone 4 closed | — |
| Frontend-neutral session projections | closed | `plans/subsystems/session-projections-roadmap.md` | M012 closed | — |
| Agent runtime, model adaptation, and ACP | closed | `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md` | M017 closed | — |
| Agent runtime correctness, autonomy, and simplification | closed | `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md` | M011/M013 follow-up closed | — |
| Agent runs, async delegation, and worktree concurrency | closed | `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md` | M009 closed | — |
| Agent convergence and independent verification | closed | `plans/subsystems/agent-convergence-roadmap.md` | M003 closed | — |
| Memory-to-skill promotion | closed | `plans/subsystems/memory-skill-promotion-hosted-verification-corrective-addendum.md` | M005 closed | — |
| Runtime consolidation, deletion, and footprint | closed | `plans/subsystems/runtime-consolidation-deletion-footprint-tui-closure-addendum.md` | M010 closed | — |
| Programmatic tool execution and Tool Programs | closed | `plans/subsystems/tool-programs-roadmap.md` | M019/M020 closed | Expansion is separately represented above; historical roadmap remains closed. |
| Development verification and release | closed | `plans/subsystems/development-verification-release-ci-reproducibility-corrective-addendum.md` | M010 closed | Operator action remains requiring `CI / verify` on `main`. |
| Distribution and installation | closed | `plans/subsystems/distribution-installation-roadmap.md` | M001/M002 closed | — |
| Runtime safety — checked edit-history corrective follow-up | closed | `plans/subsystems/runtime-safety-edit-history-corrective-addendum.md` | M013 closed | — |
| Post-audit correctness, simplification, and footprint | closed | `plans/subsystems/post-audit-correctness-simplification-daemon-lifecycle-corrective-addendum.md` | C003 closed | — |
| Search and eggsearch integration | closed | `plans/subsystems/search-eggsearch-integration-roadmap.md` | M005 closed | Historical 0.3.6-era workstream remains closed; the 0.3.9 compatibility trigger is owned by the new corrective addendum above. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies / handoff note |
|---|---|---|---|---|
| — | — | — | — | No dependency-ready implementation plans. |

## Current execution order and dependency gates

1. M009 is closing: its approved Rust 1.89 MSRV and eggsact 1.2.5 baseline adoption has landed; closure evidence is being finalized and M008's historical conditional record remains unchanged.
2. M007 is closed against the final M006 MCP representation.

Architecture convergence M009 and Runtime Safety C002 remain conditionally closed on the operational evidence listed under Blocked work. This compatibility work does not authorize a new daemon, scheduler, service bus, command router, state-management framework, verification framework, release automation, persistent search index, duplicate progressive-discovery system, or generic secret-store/DI system.

## Blocked work

| Subsystem | Milestone | Blocker |
|---|---|---|
| Architecture convergence | M009 strict operational evidence | Compatible-host root runtime / all-feature Clippy evidence. |
| Runtime safety | C002 supported-Linux evidence | Historical Landlock supported-Linux fixture evidence. |

## Closure work and current control points

Detailed historical milestone history is intentionally not duplicated here. Current foundation/control evidence relevant to active work includes:

| Subsystem | Status | Controlling evidence |
|---|---|---|
| Upstream tool-surface compatibility corrective | closing | `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md`; M008 closure record and M009 follow-up |
| TUI/frontend convergence corrective | M005-M010 closed | `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md` |
| Original multi-project TUI | M001-M004 closed | `plans/subsystems/tui-project-sessions-roadmap.md`; `plans/closure/tui-project-sessions/004-status.md` |
| Post-audit maintainability corrective | M006/M007 closed | `plans/subsystems/post-audit-maintainability-surface-corrective-addendum.md` |
| Original post-audit maintainability | M001-M005 closed | `plans/closure/post-audit-maintainability-surface/001-status.md` through `005-status.md` |
| Post-implementation maintainability closure | M001 closed | `plans/closure/post-implementation-maintainability-closure/001-status.md` |
| Frontend-neutral session projections | closed | `plans/closure/session-projections/012-status.md` |
| Agent runs/worktree concurrency | closed | `plans/closure/agent-run-worktree-concurrency/009-status.md` |
| Presence and read-only observation | M001-M003 closed | `plans/closure/presence-observation/001-status.md` through `003-status.md` |
| Project collaboration | M001-M003 closed | `plans/closure/project-collaboration/001-status.md` through `003-status.md` |
| Interactive process sessions | M001-M003 closed | `plans/subsystems/interactive-process-sessions-roadmap.md` |
| Search and eggsearch integration | closed | `plans/closure/search-eggsearch-integration/005-status.md` |
| Identity, authorization, and audit | M001-M005 closed | `plans/closure/identity-authorization-audit/001-status.md` through `005-status.md` |
| Tool Programs | closed | `plans/closure/tool-programs/019-status.md`; `plans/closure/tool-programs/020-status.md` |
| Tool Program capability expansion | M001-M003 closed | `plans/closure/tool-program-capability-expansion/001-status.md` through `003-status.md` |
| Residual runtime consolidation | M001-M003 closed | `plans/closure/residual-runtime-consolidation/001-status.md` through `003-status.md` |

## Recently closed or conditionally closed work

| Subsystem | Milestone | Status | Closure record | Implementation commit |
|---|---|---|---|---|
| Upstream tool-surface compatibility corrective | M008 Eggsact 1.2.5 in-process compatibility | conditionally closed | `plans/closure/tool-surface-upstream-compatibility/008-status.md` | `2055696` |
| Upstream tool-surface compatibility corrective | M009 Eggsact 1.2.5 MSRV adoption | closing | `plans/implementation/tool-surface-upstream-compatibility/009-eggsact-1.2.5-msrv-adoption.md` | Closure record in progress |
| Upstream tool-surface compatibility corrective | M007 Eggsearch 0.3.9 surface alignment | closed | `plans/closure/tool-surface-upstream-compatibility/007-status.md` | `0cd35ab` |
| Upstream tool-surface compatibility corrective | M006 MCP modern protocol and metadata | closed | `plans/closure/tool-surface-upstream-compatibility/006-status.md` | `65ed1c6` |
| Repository surface housekeeping corrective | M001 active surface, guard, and traceability reconciliation | closed | `plans/closure/repository-surface-housekeeping/001-status.md` | `931fb709` |
| Post-audit maintainability corrective | M006 terminal compatibility policy convergence | closed | `plans/closure/post-audit-maintainability-surface/006-status.md` | `f80b08d5` |
| Post-audit maintainability corrective | M007 MCP OAuth crypto/key lifecycle convergence | closed | `plans/closure/post-audit-maintainability-surface/007-status.md` | `aacf584` |
| TUI/frontend convergence corrective | M005 project execution context and command scope | closed | `plans/closure/tui-project-sessions/005-status.md` | `4a963e0` |
| TUI/frontend convergence corrective | M006 nonblocking session submit lifecycle | closed | `plans/closure/tui-project-sessions/006-status.md` | `ed5fb06` |
| TUI/frontend convergence corrective | M007 modal and focus state convergence | closed | `plans/closure/tui-project-sessions/007-status.md` | `3b41fea37235a26ad9e903ee7d7fc2ac61bd8c7e` |
| TUI/frontend convergence corrective | M008 App responsibility decomposition and intent/effect contract | closed | `plans/closure/tui-project-sessions/008-status.md` | `73a5a3e` |
| TUI/frontend convergence corrective | M009 keyboard sidebar and agent-tree inspector | closed | `plans/closure/tui-project-sessions/009-status.md` | `2d10e72` |
| TUI/frontend convergence corrective | M010 command discovery and keybinding convergence | closed | `plans/closure/tui-project-sessions/010-status.md` | `876a956b` |

Historical closure records MUST NOT be rewritten to conceal predecessor defects or failed verification. Corrective passes own new closure evidence rather than editing history.

## Verification policy

Verification remains deliberately light. Newly registered milestones may add focused unit/integration tests or a narrow static guard where it enforces a real ownership invariant, but they MUST NOT add new CI lanes, scanners, coverage/benchmark/binary-size gates, dependency bots, release automation, or fixed release cadence.

The upstream tool-surface corrective may use deterministic modern/legacy MCP fixtures and one optional local real-binary smoke for closure evidence. It MUST NOT add a permanent compatibility matrix, scheduled upstream smoke, network-dependent CI check, duplicate search cache, or duplicate progressive-discovery framework.

Repository-surface M001 may repair the existing project-catalog guard and use temporary census commands for documentation review, but it MUST NOT introduce a permanent docs-lint framework or network-dependent CI check.

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
- deletion of the explicitly configured legacy search fallback absent new compatibility evidence;
- automatic dependency-update bots or continuous binary-size/audit gates;
- OAuth device/provider expansion beyond the bounded MCP at-rest crypto/key migration now closed;
- generalized OAuth/provider credential-store unification absent a token-set abstraction justified by multiple consumers;
- remote workspace/node/distributed execution phases until identity/audit dependencies make them ready;
- release automation or a fixed release cadence.
