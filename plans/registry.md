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
| Long-horizon work execution | active | `plans/subsystems/long-horizon-work-execution-roadmap.md` | M001 ready; M002-M005 dependency-blocked | ADR-0003 accepted; M001 uses the closed Goal/runtime/context foundations. M002 waits for M001 closure, then M003-M005 are dependency ordered. |
| Execution reliability, approval, and autonomy | active | `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md` | M001 closed; M002 and M003 ready; M004-M008 dependency-blocked | ADR-0004 accepted. M001 provider retry closed (`plans/closure/execution-reliability-approval-autonomy/001-status.md`); M002 unified retry budget unblocked. M003 ApprovalRouter/persistence remains ready and parallel-safe. |
| Context continuity and multi-compaction coherence | closed | `plans/subsystems/context-continuity-compaction-roadmap.md` | M001+M002+M003+M004 closed | `plans/closure/context-continuity-compaction/004-status.md` |
| Dependency security and workspace consolidation | active | `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md` | M006 closed; M005 blocked (execution-path hardening landed; adoption blocked) | M001-M004 closed; M006 closed with accepted closure evidence; M005 blocked on a generalized external updater interface. |
| HTTP client consolidation and Eggfetch adoption | closed | `plans/subsystems/http-client-consolidation-roadmap.md` | M001-M003 closed | `plans/closure/http-client-consolidation/003-status.md` |
| Upstream tool-surface compatibility corrective | closed | `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md` | M009 closed | `plans/closure/tool-surface-upstream-compatibility/009-status.md` |
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
| Long-horizon work execution | M001 Goal progress and continuation correctness | ready | `plans/implementation/long-horizon-work-execution/001-goal-progress-and-continuation-correctness.md` | ADR-0003 accepted; existing Goal, RecoveryController, scheduler/run evidence and closed context-continuity foundation satisfy dependencies. |
| Execution reliability, approval, and autonomy | M001 provider retry attempt safety and taxonomy | closed | `plans/implementation/execution-reliability-approval-autonomy/001-provider-retry-attempt-safety-and-taxonomy.md` | Closure accepted at `plans/closure/execution-reliability-approval-autonomy/001-status.md`; implementation `88694706`. |
| Execution reliability, approval, and autonomy | M002 unified retry budget and side-effect reconciliation | ready | `plans/implementation/execution-reliability-approval-autonomy/002-unified-retry-budget-and-side-effect-reconciliation.md` | Hard dependency M001 closure satisfied; may proceed. |
| Execution reliability, approval, and autonomy | M003 ApprovalRouter and durable mode state | ready | `plans/implementation/execution-reliability-approval-autonomy/003-approval-router-and-durable-mode-state.md` | ADR-0004 accepted; deterministic PermissionChecker/SecurityService and daemon persistence foundations exist. May run in parallel with M001. |
| Context continuity and multi-compaction coherence | M001 durable continuation checkpoint and epoch foundation | closed | `plans/implementation/context-continuity-compaction/001-durable-continuation-checkpoint-and-epoch-foundation.md` | Closure accepted at `plans/closure/context-continuity-compaction/001-status.md`; implementation `fde6c2e3`. |
| Context continuity and multi-compaction coherence | M002 authoritative intent, plan, and frame projection | closed | `plans/implementation/context-continuity-compaction/002-authoritative-intent-plan-and-frame-projection.md` | Closure accepted at `plans/closure/context-continuity-compaction/002-status.md`; implementation `a96ed0fc`. |
| Context continuity and multi-compaction coherence | M003 bounded exact context recovery references | closed | `plans/implementation/context-continuity-compaction/003-bounded-exact-context-recovery-references.md` | Closure accepted at `plans/closure/context-continuity-compaction/003-status.md`; implementation `3ea77e9f`. |
| Dependency security and workspace consolidation | M006 Rustls advisory remediation and planning reconciliation | closed | `plans/implementation/dependency-security-workspace-consolidation/006-rustls-advisory-remediation-and-planning-reconciliation.md` | Closure accepted at `plans/closure/dependency-security-workspace-consolidation/006-status.md`; independent of blocked M005. |
| Dependency security and workspace consolidation | M001 security and duplicate graph convergence | closed | `plans/implementation/dependency-security-workspace-consolidation/001-security-and-duplicate-graph-convergence.md` | Closure accepted at `plans/closure/dependency-security-workspace-consolidation/001-status.md`; implementation `3bd54ccd`. |
| Dependency security and workspace consolidation | M002 workspace dependency ownership normalization | closed | `plans/implementation/dependency-security-workspace-consolidation/002-workspace-dependency-ownership-normalization.md` | Closure accepted at `plans/closure/dependency-security-workspace-consolidation/002-status.md`; implementation `05e7b258`. |
| Dependency security and workspace consolidation | M003 optional image feature-graph slimming | closed | `plans/implementation/dependency-security-workspace-consolidation/003-optional-image-feature-graph-slimming.md` | Closure accepted at `plans/closure/dependency-security-workspace-consolidation/003-status.md`. |
| Dependency security and workspace consolidation | M004 reusable crate boundary qualification | closed | `plans/implementation/dependency-security-workspace-consolidation/004-reusable-crate-boundary-qualification.md` | Closure accepted at `plans/closure/dependency-security-workspace-consolidation/004-status.md`; implementation `b95d37ec`. |
| Context continuity and multi-compaction coherence | M004 transactional rollover and multi-compaction qualification | closed | `plans/implementation/context-continuity-compaction/004-transactional-rollover-and-multi-compaction-qualification.md` | Closure accepted at `plans/closure/context-continuity-compaction/004-status.md`; implementation `a06b7164`. |

## Current execution order and dependency gates

1. Long-horizon M001 is dependency-ready. It should correct Goal progress/wait/no-progress behavior first; M002 durable WorkPlan follows its closure, then M003 projection/completion arbitration, M004 context-epoch integration, and M005 trajectory qualification.
2. Execution-reliability M001 is closed (provider attempt safety/taxonomy at `plans/closure/execution-reliability-approval-autonomy/001-status.md`). M002 unified retry budget is now dependency-ready and unblocked; M003 remains independently ready and parallel-safe with M002. M004/M005/M006/M007 follow the M003 policy/preference chain as defined by the roadmap. M008 is final qualification.
3. The closed context-continuity M001-M004 workstream remains the canonical compaction/rollover foundation. New long-horizon work consumes installed continuation checkpoints and does not register a context-continuity M005.
4. Dependency security/workspace M006 is closed: post-baseline RUSTSEC-2026-0285 was remediated with a targeted lock-only Rustls 0.23.41 → 0.23.45 patch (plus required `rustls-webpki` companion) and the planning control points reconciled. Closure evidence at `plans/closure/dependency-security-workspace-consolidation/006-status.md`.
5. Dependency-security M001-M004 remain closed with accepted closure evidence. M001 delivered Ratatui/LRU security convergence, DashMap 6 convergence and SQLx feature contraction; M002 established workspace version/default-policy ownership; M003 narrowed the optional image graph; M004 qualified reusable crate boundaries without speculative extraction.
6. Dependency-security M005 remains independently blocked on the generalized external updater contract. CodeGG must not copy Gregg's updater implementation or depend on greggd while that interface is absent. Its curl/shell execution-path hardening remains landed at `plans/closure/dependency-security-workspace-consolidation/005-status.md`.
7. HTTP client consolidation M001-M003 remain closed with accepted closure evidence. CodeGG's direct HTTP ownership is on published `eggfetch-core 0.1.4`; dependency-security M006 preserves its HTTP/1 + Rustls/WebPKI feature/trust profile.
8. Upstream compatibility M009 remains closed with Rust 1.89 MSRV and eggsact 1.2.5 baseline adoption.

Architecture convergence M009 and Runtime Safety C002 remain conditionally closed on the operational evidence listed under Blocked work. The new workstreams do not authorize a new daemon, scheduler, workflow engine, context-history service, authorization engine, sandbox framework, verification framework, release automation, or silent provider failover.

## Blocked work

| Subsystem | Milestone | Blocker |
|---|---|---|
| Long-horizon work execution | M002 durable WorkPlan foundation | Ordered handoff waits for M001 Goal continuation correctness closure. |
| Long-horizon work execution | M003 WorkPlan projection and completion arbiter | M002 closure. |
| Long-horizon work execution | M004 context-epoch reset and handoff integration | M003 closure plus the already-closed context-continuity foundation. |
| Long-horizon work execution | M005 long-horizon trajectory qualification | M001-M004 closure. |
| Execution reliability, approval, and autonomy | M004 selected-model/runtime-preference convergence | M003 RuntimePreferenceStore contract. |
| Execution reliability, approval, and autonomy | M005 production sandbox policy wiring | M003 execution-policy/ApprovalRouter contract. |
| Execution reliability, approval, and autonomy | M006 automatic approval reviewer | M003 + M005 closure. |
| Execution reliability, approval, and autonomy | M007 Yolo/Automatic/FullHost user surfaces | M003-M006 closure, including M004 preference convergence. |
| Execution reliability, approval, and autonomy | M008 fault-injection and reliability qualification | M001-M007 closure. |
| Dependency security and workspace consolidation | M005 generic updater interface and CodeGG adoption | M002 accepted closure satisfied; blocked on a generalized external updater package/interface that is not Gregg/greggd-specific (hardening landed; see `plans/closure/dependency-security-workspace-consolidation/005-status.md`). |
| Architecture convergence | M009 strict operational evidence | Compatible-host root runtime / all-feature Clippy evidence. |
| Runtime safety | C002 supported-Linux evidence | Historical Landlock supported-Linux fixture evidence. |

## Closure work and current control points

Detailed historical milestone history is intentionally not duplicated here. Current foundation/control evidence relevant to active work includes:

| Subsystem | Status | Controlling evidence |
|---|---|---|
| Long-horizon work execution | active; M001 ready | `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`; `plans/subsystems/long-horizon-work-execution-roadmap.md`; five plans under `plans/implementation/long-horizon-work-execution/`; existing Goal/Todo/RecoveryController/context-continuity architecture |
| Execution reliability, approval, and autonomy | active; M001 closed, M002 + M003 ready | `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`; eight plans under `plans/implementation/execution-reliability-approval-autonomy/`; `plans/closure/execution-reliability-approval-autonomy/001-status.md`; current provider/permission/security/sandbox/session-selection architecture |
| Context continuity and multi-compaction coherence | M001+M002+M003+M004 closed | `plans/subsystems/context-continuity-compaction-roadmap.md`; four implementation plans under `plans/implementation/context-continuity-compaction/`; `plans/closure/context-continuity-compaction/001-status.md`; `plans/closure/context-continuity-compaction/002-status.md`; `plans/closure/context-continuity-compaction/003-status.md`; `plans/closure/context-continuity-compaction/004-status.md`; current compaction/goal/session/artifact architecture |
| Dependency security and workspace consolidation | M006 closed; M005 blocked | `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md`; `plans/implementation/dependency-security-workspace-consolidation/006-rustls-advisory-remediation-and-planning-reconciliation.md`; M001-M006 closure records; current `Cargo.toml`/`Cargo.lock` |
| Upstream tool-surface compatibility corrective | closed | `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md`; M008 and M009 closure records |
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
| Execution reliability, approval, and autonomy | M001 provider retry attempt safety and taxonomy | closed | `plans/closure/execution-reliability-approval-autonomy/001-status.md` | `88694706` |
| Context continuity and multi-compaction coherence | M004 transactional rollover and multi-compaction qualification | closed | `plans/closure/context-continuity-compaction/004-status.md` | `a06b7164` |
| Context continuity and multi-compaction coherence | M003 bounded exact context recovery references | closed | `plans/closure/context-continuity-compaction/003-status.md` | `3ea77e9f` |
| Context continuity and multi-compaction coherence | M002 authoritative intent, plan, and frame projection | closed | `plans/closure/context-continuity-compaction/002-status.md` | `a96ed0fc` |
| Context continuity and multi-compaction coherence | M001 durable continuation checkpoint and epoch foundation | closed | `plans/closure/context-continuity-compaction/001-status.md` | `fde6c2e3` |
| Dependency security and workspace consolidation | M006 Rustls advisory remediation and planning reconciliation | closed | `plans/closure/dependency-security-workspace-consolidation/006-status.md` | `5896a127` |
| Dependency security and workspace consolidation | M004 reusable crate boundary qualification | closed | `plans/closure/dependency-security-workspace-consolidation/004-status.md` | `b95d37ec` |
| Dependency security and workspace consolidation | M002 workspace dependency ownership normalization | closed | `plans/closure/dependency-security-workspace-consolidation/002-status.md` | `05e7b258` |
| Dependency security and workspace consolidation | M003 optional image feature-graph slimming | closed | `plans/closure/dependency-security-workspace-consolidation/003-status.md` | `e9b72c56` |
| Dependency security and workspace consolidation | M001 security and duplicate graph convergence | closed | `plans/closure/dependency-security-workspace-consolidation/001-status.md` | `3bd54ccd` |
| HTTP client consolidation and Eggfetch adoption | M001 transport boundary and pinned HTTP adoption | closed | `plans/closure/http-client-consolidation/001-status.md` | `756e036`, `2a37be3` |
| HTTP client consolidation and Eggfetch adoption | M002 provider streaming and Eggpool adoption | closed | `plans/closure/http-client-consolidation/002-status.md` | `42dc22a` |
| HTTP client consolidation and Eggfetch adoption | M003 remaining HTTP consumers and reqwest retirement | closed | `plans/closure/http-client-consolidation/003-status.md` | `ff448fc` |
| Upstream tool-surface compatibility corrective | M008 Eggsact 1.2.5 in-process compatibility | conditionally closed | `plans/closure/tool-surface-upstream-compatibility/008-status.md` | `2055696` |
| Upstream tool-surface compatibility corrective | M009 Eggsact 1.2.5 MSRV adoption | closed | `plans/closure/tool-surface-upstream-compatibility/009-status.md` | `dc0f915` |
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

Historical closure records MUST NOT be rewritten to conceal predecessor defects or failed verification. Corrective/new-evidence passes own new closure evidence rather than editing history.

## Verification policy

Verification remains deliberately light. Newly registered milestones may add focused unit/integration tests or a narrow static guard where it enforces a real ownership invariant, but they MUST NOT add new CI lanes, scanners, coverage/benchmark/binary-size gates, dependency bots, release automation, or fixed release cadence.

The long-horizon work roadmap may add deterministic WorkPlan/Goal/Todo/context-transition/restart scenarios and force small context limits with scripted providers. It MUST NOT add live-provider CI, a second history/compaction store, a generic workflow engine, or an unbounded plan dump/benchmark gate.

The execution-reliability/approval roadmap may add deterministic provider fault streams, retry/side-effect fixtures, permission/reviewer/sandbox matrices, preference restart tests, and the existing supported-Linux sandbox fixture. It MUST NOT add live-provider CI, a permanent chaos service, a new authorization engine, a second scheduler, a new cross-platform sandbox framework, or network-containment claims without an actual backend.

The dependency security/workspace roadmap may use `cargo audit`, `cargo tree -d`, reverse dependency trees, feature trees, package dry-runs, and `cargo bloat` as temporary local/closure evidence. It MUST NOT turn advisory status, duplicate counts, package counts, or artifact size into new continuous CI gates. M001 may update an existing explicit audit ignore only when reachability/applicability evidence changes. M002 must prove feature equivalence rather than centralizing maximal feature unions. M004 may run `cargo package` dry-runs but MUST NOT publish automatically. M005 remains blocked until its external package interface exists. M006 may perform only targeted Rustls/required-companion updates and focused Eggfetch/provider/TLS-consumer verification; it MUST NOT add an advisory ignore, broad lockfile update, TLS framework, or public-network CI test.

The HTTP client consolidation roadmap may use deterministic loopback HTTP/TLS/SSE fixtures plus temporary `rg`/`cargo tree` dependency censuses for closure evidence. It MUST NOT add network-dependent CI, a permanent dependency scanner, a binary-size threshold, or a generic HTTP abstraction solely for verification.

The upstream tool-surface corrective may use deterministic modern/legacy MCP fixtures and one optional local real-binary smoke for closure evidence. It MUST NOT add a permanent compatibility matrix, scheduled upstream smoke, network-dependent CI check, duplicate search cache, or duplicate progressive-discovery framework.

Repository-surface M001 may repair the existing project-catalog guard and use temporary census commands for documentation review, but it MUST NOT introduce a permanent docs-lint framework or network-dependent CI check.

The context-continuity roadmap may add focused deterministic checkpoint-store, compaction, restart, cancellation, and repeated-trajectory tests. Its closure may force small effective context limits with local fake providers, but MUST NOT add live-provider/network CI, a new benchmark gate, a vector-history service, or another permanent verification framework.

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
- production hosted Tool Program transport;
- seccomp, namespace, container, or remote-execution sandbox expansion;
- persistent search indexing;
- deletion of the explicitly configured legacy search fallback absent new compatibility evidence;
- automatic dependency-update bots or continuous binary-size/audit gates;
- OAuth device/provider expansion beyond the bounded MCP at-rest crypto/key migration now closed;
- generalized OAuth/provider credential-store unification absent a token-set abstraction justified by multiple consumers;
- remote workspace/node/distributed execution phases until identity/audit dependencies make them ready;
- release automation or a fixed release cadence;
- upstreaming the validated-destination/SSRF policy into Eggfetch or creating a generic network-policy crate before a second independent consumer justifies that boundary;
- replacing Axum/Tower with Eggserve or adding Eggress/greggd solely for Eggstack component uniformity.