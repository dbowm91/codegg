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
| Dependency security and workspace consolidation | active | `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md` | M006 closed; M005 blocked (execution-path hardening landed; adoption blocked) | M001-M004 closed; M006 closed with accepted closure evidence; M005 blocked on a generalized external updater interface. |
| Architecture convergence and incomplete verticals | conditionally closed | `plans/subsystems/architecture-convergence-strict-closure-corrective-addendum.md` | M009 conditionally closed | Compatible-host root runtime and strict all-feature Clippy evidence remains outstanding. |
| Runtime safety, resource control, and footprint | conditionally closed | `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` | C002 conditionally closed | Supported-Linux Landlock fixture evidence remains outstanding. |
| Agent context and discovery surface | closed | `plans/subsystems/agent-context-discovery-surface-roadmap.md` | M001 + M002 closed | Context discovery workstream closed with MCP projection and scoped curated-memory evidence. |
| Plugin ecosystem and harness interoperability | closed | `plans/subsystems/plugin-ecosystem-interoperability-roadmap.md` | M001-M004 closed | Plugin interoperability workstream closed with portable import, browser bundle, catalog, and host-install evidence. |
| Tool-selection advisor | closed | `plans/subsystems/tool-selection-advisor-roadmap.md` | M001-M005 closed | Accepted ADR-0009; optional local advisor path, training lifecycle, consent gates, and qualification evidence recorded. |
| Tool-selection advisor post-closure corrective | active | `plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md` | M001 closed; M002 + M003 ready; M004 gated | Corrects smoke-scale data/consent ambiguity, replaces lexical baseline with real contextual encoder work, and moves promotion to pre-turn provider disclosure. Original M001-M005 closures remain historical. |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies / handoff note |
|---|---|---|---|---|
| Agent context and discovery surface | M001 MCP resource/prompt projection | closed | `plans/implementation/agent-context-discovery-surface/001-mcp-resource-and-prompt-projection.md` | Closure: `plans/closure/agent-context-discovery-surface/001-status.md`. |
| Agent context and discovery surface | M002 bounded persistent-memory retrieval | closed | `plans/implementation/agent-context-discovery-surface/002-bounded-persistent-memory-retrieval.md` | Closure: `plans/closure/agent-context-discovery-surface/002-status.md`; transcript/vector search remains out of scope. |
| Plugin ecosystem and harness interoperability | M001 first-class plugin tools | closed | `plans/implementation/plugin-ecosystem-interoperability/001-first-class-plugin-tools.md` | Closure: `plans/closure/plugin-ecosystem-interoperability/001-status.md`. |
| Plugin ecosystem and harness interoperability | M002 portable Agent Plugins import | closed | `plans/implementation/plugin-ecosystem-interoperability/002-portable-agent-plugin-import.md` | Closure: `plans/closure/plugin-ecosystem-interoperability/002-status.md`. |
| Plugin ecosystem and harness interoperability | M003 Playwright browser-testing integration | closed | `plans/implementation/plugin-ecosystem-interoperability/003-playwright-browser-integration-bundle.md` | Closure: `plans/closure/plugin-ecosystem-interoperability/003-status.md`. |
| Plugin ecosystem and harness interoperability | M004 extension catalog/install | closed | `plans/implementation/plugin-ecosystem-interoperability/004-extension-catalog-and-install.md` | Closure: `plans/closure/plugin-ecosystem-interoperability/004-status.md`. |
| Tool-selection advisor | M005 advisor integration/qualification | closed | `plans/implementation/tool-selection-advisor/005-advisor-integration-and-qualification.md` | Closure: `plans/closure/tool-selection-advisor/005-status.md`; no later advisor milestone remains blocked. |
| Tool-selection advisor post-closure corrective | M001 dataset/split/consent integrity | closed | `plans/implementation/tool-selection-advisor-post-closure-corrective/001-dataset-split-and-consent-integrity.md` | Closure: `plans/closure/tool-selection-advisor-post-closure-corrective/001-status.md`; implementation `66bf63e`. |
| Tool-selection advisor post-closure corrective | M002 contextual encoder runtime/training | ready | `plans/implementation/tool-selection-advisor-post-closure-corrective/002-contextual-encoder-runtime-and-training.md` | M001 closed with frozen corpus/splits/consent; framework spike and encoder work may begin. |
| Tool-selection advisor post-closure corrective | M003 pre-turn proactive disclosure | ready | `plans/implementation/tool-selection-advisor-post-closure-corrective/003-pre-turn-proactive-tool-disclosure.md` | Independent of encoder training; use existing ToolAdvisor/test advisor to wire the correct request-preparation seam. |

## Current execution order and dependency gates

**Team collaboration corrective gate:** Historical M001-M006 remain closed with their original closure records. The post-closure follow-up `plans/subsystems/team-collaboration-post-closure-corrective-addendum.md` is complete: M001 registration authority boundary is `closed`; M002 Workspace task-cancellation ownership is `closed`; M003 verification-guard convergence is `closed` (`plans/closure/team-collaboration-post-closure-corrective/003-status.md`; implementation `23ad6937`). The predecessor closure records remain immutable historical evidence; final correctness for the three post-closure findings is controlled by the post-closure corrective.

**Post-closure cleanup gates:** Command-surface/Security-Review C001 and its separately registered WorkOrder, managed-key, and migration-test correctives are formally closed with hosted `CI / verify` run 35483642396. `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md` M001+M002+M003+M004 are closed (`plans/closure/identity-audit-live-execution-post-closure-corrective/001-status.md` + `002-status.md` + `003-status.md` + `004-status.md`; implementations `7c75c009` + `e0cc7f05` + `591cb4de` + `42b8f9ec`). The workstream is complete. These plans do not reopen closed collaboration scope or distributed node/remote audit.

**Tool-selection advisor corrective gate:** Accepted `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` remains controlling and the original `plans/subsystems/tool-selection-advisor-roadmap.md` M001-M005 closure records remain immutable historical evidence. Post-closure review found that `hashed-linear-v1` is only a lexical baseline rather than the planned contextual encoder, `promote` is currently wired reactively inside `tool_search` rather than before provider-definition finalization, qualification remains smoke-scale/no-live-primary-model, the `tool-advisor` Cargo feature does not yet isolate a neural runtime, and training-event consent has two representations. M001 is closed at `plans/closure/tool-selection-advisor-post-closure-corrective/001-status.md`; M002 is unblocked and ready; M003 remains ready and independent; M004 remains blocked on M002+M003. Advisor use, model use, training, capture, and remote telemetry all remain opt-in/default-off.

**Agent extension/interoperability closure state:** `plans/subsystems/agent-context-discovery-surface-roadmap.md` is closed with M001 MCP resource/prompt projection and M002 bounded curated-memory retrieval closed. `plans/subsystems/plugin-ecosystem-interoperability-roadmap.md` is closed with M001 first-class plugin tools, M002 Agent Plugins 1.0 import, M003 Playwright browser-testing integration, and M004 extension catalog/install closed. Closure evidence is recorded under `plans/closure/agent-context-discovery-surface/` and `plans/closure/plugin-ecosystem-interoperability/`. No milestone in either workstream remains ready or blocked. This closure does not reopen the closed context-continuity transcript/vector-search scope or the closed coding-agent tool-surface corrective milestones.

1. Project Work Orders and Task View M001+M002+M003+M004+M005+M006+M007 remain closed under accepted ADR-0005 (M001: durable WorkOrder/Occurrence/SequenceLane domain, project-scoped authorization/protocol, CAS mutation foundation, no execution; M002: release coordinator with exactly-once claim/materialization into ordinary sessions/jobs through `JobSubmissionService`, managed-worktree isolation, model/policy narrowing; M003: Task composer/scheduling sheet/project Task view with CAS reorder and separately-scoped Task-model preference, `/tasks` migration; M004: global Workspace dashboard with bounded authorized project/task/session/attention projection, `/workspace` + hotkey, lazy detail, team privacy, event/reconnect/revocation guards; M005: narrow external task-trigger capability with verifier-only storage, authenticated/idempotent POST fire endpoint, expiry/max-fire/revocation, replay/race safety, plus the `ConnectInfo` server-wiring repair found by its HTTP qualification; M006: dedicated agent WorkOrder tool with atomic sequential batches; M007: harness-only end-to-end trajectory/recovery/contention/security qualification with no production delta). Closure evidence remains at `plans/closure/project-work-orders-task-view/001-status.md` through `007-status.md`. The post-closure UX-fidelity corrective C001 is closed at `plans/closure/project-work-orders-task-view-corrective/001-status.md` (implementation `1bd77c6d`): bare-Tab Session↔Task selection, focused direct queue reorder, and human external-trigger setup/one-time-secret recovery landed without reopening backend ownership. Final user-facing behavior is controlled by C001 wherever it differs from the original M003/M005/M007 planning text; those predecessor closures remain historical evidence and MUST NOT be rewritten or treated as open.
2. Long-horizon M001+M002+M003+M004+M005 are closed (Goal progress/wait/no-progress correctness at `plans/closure/long-horizon-work-execution/001-status.md`; durable WorkPlan foundation at `plans/closure/long-horizon-work-execution/002-status.md`; projection/completion arbitration at `plans/closure/long-horizon-work-execution/003-status.md`; context-epoch reset/handoff at `plans/closure/long-horizon-work-execution/004-status.md`; trajectory qualification at `plans/closure/long-horizon-work-execution/005-status.md`). The workstream is closed.
3. Execution-reliability M001+M002+M003+M004+M005+M006+M007+M008 are closed (provider attempt safety/taxonomy at `plans/closure/execution-reliability-approval-autonomy/001-status.md`; unified retry budget at `plans/closure/execution-reliability-approval-autonomy/002-status.md`; ApprovalRouter/persistence at `plans/closure/execution-reliability-approval-autonomy/003-status.md`; model/preference convergence at `plans/closure/execution-reliability-approval-autonomy/004-status.md`; sandbox wiring at `plans/closure/execution-reliability-approval-autonomy/005-status.md`; automatic approval reviewer at `plans/closure/execution-reliability-approval-autonomy/006-status.md`; Yolo/Automatic/FullHost user surfaces at `plans/closure/execution-reliability-approval-autonomy/007-status.md`; fault-injection qualification at `plans/closure/execution-reliability-approval-autonomy/008-status.md`). The workstream is closed.
4. The closed context-continuity M001-M004 workstream remains the canonical compaction/rollover foundation. New long-horizon work consumes installed continuation checkpoints and does not register a context-continuity M005.
5. Dependency security/workspace M006 is closed: post-baseline RUSTSEC-2026-0285 was remediated with a targeted lock-only Rustls 0.23.41 → 0.23.45 patch (plus required `rustls-webpki` companion) and the planning control points reconciled. Closure evidence at `plans/closure/dependency-security-workspace-consolidation/006-status.md`.
6. Dependency-security M001-M004 remain closed with accepted closure evidence. M001 delivered Ratatui/LRU security convergence, DashMap 6 convergence and SQLx feature contraction; M002 established workspace version/default-policy ownership; M003 narrowed the optional image graph; M004 qualified reusable crate boundaries without speculative extraction.
7. Dependency-security M005 remains independently blocked on the generalized external updater contract. CodeGG must not copy Gregg's updater implementation or depend on greggd while that interface is absent. Its curl/shell execution-path hardening remains landed at `plans/closure/dependency-security-workspace-consolidation/005-status.md`.
8. HTTP client consolidation M001-M003 remain closed with accepted closure evidence. CodeGG's direct HTTP ownership is on published `eggfetch-core 0.1.4`; dependency-security M006 preserves its HTTP/1 + Rustls/WebPKI feature/trust profile.
9. HTTP client maintenance consolidation M001 is closed (supported Eggfetch floor 0.1.5, generic bounded-response enforcement transferred to Eggfetch, repeated ordinary-client construction policy consolidated without a second HTTP abstraction or retry owner). Closure evidence at `plans/closure/http-client-maintenance-consolidation/001-status.md`. No future plan lists it as a prerequisite.
10. Upstream compatibility M009 remains closed with Rust 1.89 MSRV and eggsact 1.2.5 baseline adoption.
11. Self-contained installation M001+M002 are closed: release archives carry the exact managed runfile bundle (`codegg`, `codegg-sandbox-helper`, `codegg-eggsearch`, plus optional fixed `THIRD-PARTY-NOTICES.txt`; explicit `.exe` names on Windows) with pinned eggsearch 0.3.9 provenance and transactional backup/rollback (M001 closure at `plans/closure/self-contained-installation-corrective/001-status.md`, implementation `f9ec8602`); default runtime resolution consumes the bundle via the shared installation sibling rule with explicit overrides preserved, installation/runfile diagnostics land in `codegg doctor installation`/`search`, eggsact stays in-process 1.2.5, and the temporary-home clean-host harness qualifies the install → `codegg` → doctor → `/connect`-proxy path with no Rust, no external eggsearch/eggsact, and no master-key env vars (M002 closure at `plans/closure/self-contained-installation-corrective/002-status.md`, implementation `cf7a06bb`). The corrective workstream is closed.

Architecture convergence M009 and Runtime Safety C002 remain conditionally closed on the operational evidence listed under Blocked work. The new workstreams do not authorize a new daemon, scheduler, workflow engine, context-history service, authorization engine, sandbox framework, verification framework, release automation, or silent provider failover.

## Blocked work

| Subsystem | Milestone | Blocker |
|---|---|---|
| Dependency security and workspace consolidation | M005 generic updater interface and CodeGG adoption | M002 accepted closure satisfied; blocked on a generalized external updater package/interface that is not Gregg/greggd-specific (hardening landed; see `plans/closure/dependency-security-workspace-consolidation/005-status.md`). |
| Architecture convergence | M009 strict operational evidence | Compatible-host root runtime / all-feature Clippy evidence. |
| Runtime safety | C002 supported-Linux evidence | Historical Landlock supported-Linux fixture evidence. |
| Tool-selection advisor post-closure corrective | M004 live small-model trajectory qualification | M001 + M002 + M003 closure; requires corrected dataset/consent, contextual encoder, and true pre-turn disclosure. |

## Closure work and current control points

Detailed historical milestone history is intentionally not duplicated here. Current foundation/control evidence relevant to active work includes:

| Subsystem | Status | Controlling evidence |
|---|---|---|
| Project Work Orders and Task View — UX fidelity corrective | C001 closed | `plans/subsystems/project-work-orders-task-view-ux-corrective-addendum.md`; `plans/implementation/project-work-orders-task-view-corrective/001-human-task-ux-and-trigger-surface.md`; `plans/closure/project-work-orders-task-view-corrective/001-status.md` (implementation `1bd77c6d`); predecessor M003/M005/M007 closure records; accepted ADR-0005/ADR-0004 |
| Provider /connect restoration — managed-key CI corrective | C001 closed | `plans/subsystems/provider-connect-restoration-ci-corrective-addendum.md`; `plans/implementation/provider-connect-restoration-ci-corrective/001-managed-key-concurrency-ci-corrective.md`; `plans/closure/provider-connect-restoration-ci-corrective/001-status.md`; hosted run 35483642396 |
| Workspace migration test-contract CI corrective | C001 closed | `plans/subsystems/workspace-migration-test-contract-ci-corrective-addendum.md`; `plans/implementation/workspace-migration-test-contract-ci-corrective/001-canonical-storage-layout-assertions.md`; `plans/closure/workspace-migration-test-contract-ci-corrective/001-status.md`; hosted run 35483642396 |
| Coding-agent tool surface corrective | M001-M007 closed | `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md`; `plans/closure/coding-agent-tool-surface-corrective/001-status.md` through `007-status.md`; M005 implementation `b31ce65`; no future plan in this workstream remains blocked on it |
| Project Work Orders and Task View | M001+M002+M003+M004+M005+M006+M007 closed | `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`; `plans/subsystems/project-work-orders-task-view-roadmap.md`; seven implementation plans under `plans/implementation/project-work-orders-task-view/`; `plans/closure/project-work-orders-task-view/001-status.md`; `plans/closure/project-work-orders-task-view/002-status.md`; `plans/closure/project-work-orders-task-view/003-status.md`; `plans/closure/project-work-orders-task-view/004-status.md`; `plans/closure/project-work-orders-task-view/005-status.md`; `plans/closure/project-work-orders-task-view/006-status.md`; `plans/closure/project-work-orders-task-view/007-status.md`; existing project/session/jobs/scheduler/worktree/authorization/runtime-preference/WorkPlan foundations |
| Long-horizon work execution | closed; M001+M002+M003+M004+M005 closed | `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`; `plans/subsystems/long-horizon-work-execution-roadmap.md`; five plans under `plans/implementation/long-horizon-work-execution/`; `plans/closure/long-horizon-work-execution/001-status.md`; `plans/closure/long-horizon-work-execution/002-status.md`; `plans/closure/long-horizon-work-execution/003-status.md`; `plans/closure/long-horizon-work-execution/004-status.md`; `plans/closure/long-horizon-work-execution/005-status.md`; existing Goal/Todo/RecoveryController/context-continuity architecture |
| Execution reliability, approval, and autonomy | closed; M001+M002+M003+M004+M005+M006+M007+M008 closed | `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`; eight plans under `plans/implementation/execution-reliability-approval-autonomy/`; `plans/closure/execution-reliability-approval-autonomy/001-status.md`; `plans/closure/execution-reliability-approval-autonomy/002-status.md`; `plans/closure/execution-reliability-approval-autonomy/003-status.md`; `plans/closure/execution-reliability-approval-autonomy/004-status.md`; `plans/closure/execution-reliability-approval-autonomy/005-status.md`; `plans/closure/execution-reliability-approval-autonomy/006-status.md`; `plans/closure/execution-reliability-approval-autonomy/007-status.md`; `plans/closure/execution-reliability-approval-autonomy/008-status.md`; current provider/permission/security/sandbox/session-selection architecture |
| Context continuity and multi-compaction coherence | M001+M002+M003+M004 closed | `plans/subsystems/context-continuity-compaction-roadmap.md`; four implementation plans under `plans/implementation/context-continuity-compaction/`; `plans/closure/context-continuity-compaction/001-status.md`; `plans/closure/context-continuity-compaction/002-status.md`; `plans/closure/context-continuity-compaction/003-status.md`; `plans/closure/context-continuity-compaction/004-status.md`; current compaction/goal/session/artifact architecture |
| Dependency security and workspace consolidation | M006 closed; M005 blocked | `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md`; `plans/implementation/dependency-security-workspace-consolidation/006-rustls-advisory-remediation-and-planning-reconciliation.md`; M001-M006 closure records; current `Cargo.toml`/`Cargo.lock` |
| HTTP client maintenance consolidation | M001 closed | `plans/subsystems/http-client-maintenance-consolidation-roadmap.md`; `plans/implementation/http-client-maintenance-consolidation/001-eggfetch-0.1.5-policy-and-body-ownership.md`; closed original HTTP-client consolidation M001-M003 as predecessor evidence; `plans/closure/http-client-maintenance-consolidation/001-status.md` |
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
| Tool-selection advisor post-closure corrective | M001 dataset/split/consent integrity | closed | `plans/closure/tool-selection-advisor-post-closure-corrective/001-status.md` | `66bf63e` |
| Coding-agent tool surface corrective | M001 surface authority and discovery correctness | closed | `plans/closure/coding-agent-tool-surface-corrective/001-status.md` | `fd63425` |
| Coding-agent tool surface corrective | M002 coding profile and contextual capability exposure | closed | `plans/closure/coding-agent-tool-surface-corrective/002-status.md` | `32d152a` |
| Coding-agent tool surface corrective | M003 compact discovery and multiplexed-tool ergonomics | closed | `plans/closure/coding-agent-tool-surface-corrective/003-status.md` + M007 supplemental `plans/closure/coding-agent-tool-surface-corrective/007-status.md` | `94cdaaf` + `4ac7d7e7` |
| Coding-agent tool surface corrective | M004 structured verification facade | closed | `plans/closure/coding-agent-tool-surface-corrective/004-status.md` + M007 supplemental `plans/closure/coding-agent-tool-surface-corrective/007-status.md` | `a978484` + `4ac7d7e7` |
| Coding-agent tool surface corrective | M007 M003/M004 closure evidence reconciliation | closed | `plans/closure/coding-agent-tool-surface-corrective/007-status.md` | evidence-only; `4ac7d7e7` |
| Coding-agent tool surface corrective | M006 LSP preview runtime and authority seam | closed | `plans/closure/coding-agent-tool-surface-corrective/006-status.md` | `4cf35238` |
| Coding-agent tool surface corrective | M005 checked LSP preview application | closed | `plans/closure/coding-agent-tool-surface-corrective/005-status.md` | `b31ce65` |
| Coding-agent tool surface — post-closure evidence polish | M001 LSP preview apply concurrency and restart evidence | closed | `plans/closure/coding-agent-tool-surface-post-closure-evidence-polish/001-status.md` | `730b582b` |
| Team collaboration corrective | M006 multi-user trajectory/security qualification | closed | `plans/closure/team-collaboration-corrective/006-status.md` | harness-only (see closure) |
| Team collaboration corrective | M005 Workspace selected-project chat view | closed | `plans/closure/team-collaboration-corrective/005-status.md` | `86217a7d` |
| Team collaboration corrective | M004 shared-session controller lease | closed | `plans/closure/team-collaboration-corrective/004-status.md` | `1bc967c1` |
| Team collaboration corrective | M003 team membership and device-token administration | closed | `plans/closure/team-collaboration-corrective/003-status.md` | `f7afefb1` |
| Team collaboration corrective | M002 project/channel chat access policy | closed | `plans/closure/team-collaboration-corrective/002-status.md` | `2f4bec32` |
| Team collaboration corrective | M001 network API authorization convergence | closed | `plans/closure/team-collaboration-corrective/001-status.md` | `cc41e4a4` |
| Team collaboration post-closure corrective | M001 registration authority boundary | closed | `plans/closure/team-collaboration-post-closure-corrective/001-status.md` | `02971be8` |
| Team collaboration post-closure corrective | M002 Workspace task cancellation ownership | closed | `plans/closure/team-collaboration-post-closure-corrective/002-status.md` | `06d92a17` |
| Team collaboration post-closure corrective | M003 verification guard convergence | closed | `plans/closure/team-collaboration-post-closure-corrective/003-status.md` | `23ad6937` |
| Self-contained installation corrective | M002 runtime resolution and clean-host qualification | closed | `plans/closure/self-contained-installation-corrective/002-status.md` | `cf7a06bb` |
| Self-contained installation corrective | M001 managed runfile release bundle | closed | `plans/closure/self-contained-installation-corrective/001-status.md` | `f9ec8602` |
| Provider /connect restoration corrective | M003 provider-neutral /connect TUI | closed | `plans/closure/provider-connect-restoration-corrective/003-status.md` | `5cc460c0` |
| Provider /connect restoration corrective | M002 provider catalog and neutral provisioning | closed | `plans/closure/provider-connect-restoration-corrective/002-status.md` | `b532ef84` + `189eb37b` |
| Provider /connect restoration corrective | M001 first-run credential key bootstrap | closed | `plans/closure/provider-connect-restoration-corrective/001-status.md` | `568cae33` |
| Command surface / Security Review post-closure corrective | C001 Security Review show contract and CI closure | closed | `plans/closure/command-surface-security-review-ci-post-closure-corrective/001-status.md` | `4ec46a9b` + `5c4e2966`; hosted closure run `35483642396` |
| Identity / audit live execution post-closure corrective | M001 trusted execution audit context and emitter | closed | `plans/closure/identity-audit-live-execution-post-closure-corrective/001-status.md` | `7c75c009` |
| Identity / audit live execution post-closure corrective | M002 command, interactive-process, and Git live audit hooks | closed | `plans/closure/identity-audit-live-execution-post-closure-corrective/002-status.md` | `e0cc7f05` |
| Identity / audit live execution post-closure corrective | M003 scheduler terminal job-completion audit | closed | `plans/closure/identity-audit-live-execution-post-closure-corrective/003-status.md` | `591cb4de` |
| Project Work Orders and Task View — CI migration-test corrective | C001 migration version test contract | closed | `plans/closure/project-work-orders-task-view-ci-corrective/001-status.md` | `5c4e2966`; hosted closure run `35483642396` |
| Provider /connect restoration — managed-key CI corrective | C001 managed-key concurrent first-write closure | closed | `plans/closure/provider-connect-restoration-ci-corrective/001-status.md` | `c9c37cca`; hosted closure run `35483642396` |
| Workspace migration test-contract CI corrective | C001 canonical storage-layout assertions | closed | `plans/closure/workspace-migration-test-contract-ci-corrective/001-status.md` | `c9c37cca`; hosted closure run `35483642396` |
| Command surface reconciliation corrective | M002 CLI surface cleanup | closed | `plans/closure/command-surface-reconciliation-corrective/002-status.md` | `da7fab03` |
| Command surface reconciliation corrective | M001 TUI command action convergence | closed | `plans/closure/command-surface-reconciliation-corrective/001-status.md` | `6e5203b6` |
| Project Work Orders and Task View — UX fidelity corrective | C001 human Task UX and trigger surface | closed | `plans/closure/project-work-orders-task-view-corrective/001-status.md` | `1bd77c6d` |
| Project Work Orders and Task View | M007 trajectory/recovery/security qualification | closed | `plans/closure/project-work-orders-task-view/007-status.md` | `0216e814` |
| Project Work Orders and Task View | M006 agent WorkOrder tool and atomic batches | closed | `plans/closure/project-work-orders-task-view/006-status.md` | `38e533a8` |
| Project Work Orders and Task View | M005 external task-trigger capability and endpoint | closed | `plans/closure/project-work-orders-task-view/005-status.md` | `f22c9d8d` |
| Project Work Orders and Task View | M004 global Workspace dashboard and team-aware task projection | closed | `plans/closure/project-work-orders-task-view/004-status.md` | `8c6e8190` |
| Project Work Orders and Task View | M003 project Task composer, scheduling sheet, and task/session view | closed | `plans/closure/project-work-orders-task-view/003-status.md` | `79a00460` |
| Project Work Orders and Task View | M001 WorkOrder domain, storage, authorization, and protocol | closed | `plans/closure/project-work-orders-task-view/001-status.md` | `a856f2e0` |
| Project Work Orders and Task View | M002 release coordinator and session materialization | closed | `plans/closure/project-work-orders-task-view/002-status.md` | `c54656e5` |
| Long-horizon work execution | M005 long-horizon trajectory and recovery qualification | closed | `plans/closure/long-horizon-work-execution/005-status.md` | `35d8a955` |
| Long-horizon work execution | M001 Goal progress and continuation correctness | closed | `plans/closure/long-horizon-work-execution/001-status.md` | `5d79bd9d` |
| Long-horizon work execution | M002 durable WorkPlan foundation | closed | `plans/closure/long-horizon-work-execution/002-status.md` | `0aceb37d` |
| Long-horizon work execution | M003 WorkPlan projection and completion arbiter | closed | `plans/closure/long-horizon-work-execution/003-status.md` | `2ff09bef` |
| Long-horizon work execution | M004 context-epoch reset and handoff integration | closed | `plans/closure/long-horizon-work-execution/004-status.md` | `e3bfc565` |
| Execution reliability, approval, and autonomy | M008 fault-injection and reliability qualification | closed | `plans/closure/execution-reliability-approval-autonomy/008-status.md` | `f9e37930` |
| Execution reliability, approval, and autonomy | M007 Yolo/Automatic/FullHost user surfaces | closed | `plans/closure/execution-reliability-approval-autonomy/007-status.md` | `36f0f48d` |
| Execution reliability, approval, and autonomy | M006 automatic approval reviewer | closed | `plans/closure/execution-reliability-approval-autonomy/006-status.md` | `dd2ddd1a` |
| Execution reliability, approval, and autonomy | M005 production sandbox policy wiring | closed | `plans/closure/execution-reliability-approval-autonomy/005-status.md` | `34ceffe5` |
| Execution reliability, approval, and autonomy | M004 selected-model and runtime-preference convergence | closed | `plans/closure/execution-reliability-approval-autonomy/004-status.md` | `2842a25a` |
| Execution reliability, approval, and autonomy | M003 ApprovalRouter and durable mode state | closed | `plans/closure/execution-reliability-approval-autonomy/003-status.md` | `79cac319` |
| Execution reliability, approval, and autonomy | M002 unified retry budget and side-effect reconciliation | closed | `plans/closure/execution-reliability-approval-autonomy/002-status.md` | `8d810fc3` |
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
| HTTP client maintenance consolidation | M001 Eggfetch 0.1.5 policy and body ownership consolidation | closed | `plans/closure/http-client-maintenance-consolidation/001-status.md` | `72f6610d` |
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

The Project Work Orders and Task View roadmap may add deterministic SQLite/restart, release-gate, sequence-CAS, managed-worktree, trigger-replay, authorization/privacy, and bounded TUI/tool trajectory fixtures. It MUST NOT add live-provider/network CI, a second scheduler/background loop, a generic workflow/BPM engine, an unbounded recurrence language, a new authorization framework, or a permanent chaos/benchmark framework. External-trigger tests should use loopback/in-process fixtures; M007 should reuse existing deterministic test infrastructure rather than create a broad new verification harness. The UX-fidelity corrective C001 may add focused keybinding/composer, scheduling-queue, one-time-trigger-secret, stale-response/recovery, and trigger-management TUI tests while reusing M003/M005/M007 suites and existing trigger/ownership guards; it MUST NOT add a new UI harness framework, secrets framework, public-network test, or CI lane.

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
