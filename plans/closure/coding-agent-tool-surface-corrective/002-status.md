# Coding-Agent Tool Surface Corrective M002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/coding-agent-tool-surface-corrective/002-coding-profile-and-contextual-capability-exposure.md`
Source subsystem roadmap: `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m002--coding-profile-and-contextual-capability-exposure`
Repository baseline reviewed: `32d152a`
Implementation commits: `32d152a` — complete coding palettes and host-derived contextual Goal/WorkPlan/context recovery exposure

## 1. Executive finding

M002 is complete and strictly closed. Supported coding profiles retain a native inspect/search, edit/create, controlled-command, supervised-test, and context-recovery loop. Active Goal and WorkPlan state is resolved from canonical stores once during request preparation and promotes only bounded relevant tools. WorkOrder remains a distinct project-bound capability and is not inferred from user prose.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Normal coding profiles retain native creation and verification | Curated/Minimal palette regression requires read, repo_search, edit, write, bash, test, and tool_search | Pass |
| Context recovery is actionable | context_read is registered only with the configured artifact store and is retained in coding palettes; contextual helper regression covers availability | Pass |
| Active Goal/WorkPlan tools are contextual | Host-state helper and request-preparation cache revision use active Goal/WorkPlan store state; inactive state remains unpromoted | Pass |
| WorkOrder remains distinct and discoverable | M001 live discovery plus M002 contextual regression explicitly excludes work_order from prompt-state guessing | Pass |
| Policy/disabled/parent/plan restrictions remain authoritative | M001 resolved-surface contract, existing negative suites, quick verification | Pass |
| No new execution/state owner | Existing native tools, stores, ToolBroker, scheduler, and context artifact store remain owners | Pass |

## 3. Production implementation evidence

- Curated and Minimal palettes now include native write/create, supervised test, and context_read paths; LSP is retained in Curated where the backend/profile gate permits it.
- Initial palette projection accepts a bounded host-derived contextual set. Allowed tools outside the palette become deferred rather than disappearing from discovery.
- Request preparation resolves artifact recovery availability, active Goal, and active WorkPlan once and folds the result into the tool-definition cache key.
- Goal and WorkPlan tools are promoted only for active session-bound state. WorkOrder is registered only with functional project scope and remains deferred by default.

## 4. Verification executed (local)

- `cargo check -p codegg --lib` — passed.
- focused palette and contextual disclosure regressions — passed.
- `cargo test --test tool_surface_minimization -- --test-threads=1` — 13 passed.
- `cargo test --test compaction -- --test-threads=1` — 65 passed (current target; plan's `context_compaction` name is absent).
- `cargo test --test work_plan_projection_arbiter -- --test-threads=1` — 9 passed.
- `cargo test --test work_orders_m006_agent_tool -- --test-threads=1` — 21 passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `scripts/verify.sh quick` — passed.
- `git diff --check` — passed.

## 5. Invariant review

Exposure remains a projection over the M001 policy-allowed universe and cannot widen authority. Contextual promotion is per-turn snapshot behavior; changes after preparation appear on the next turn. WorkOrder is not conflated with TaskTool delegation.

## 6. Failure and recovery review

Goal/WorkPlan lookup failure yields smaller disclosure rather than fabricated active state. Artifact handles are only useful where the configured artifact store and registered context_read path exist. No new runtime or retry path was introduced.

## 7. Migration and compatibility review

No storage, protocol, or profile-schema migration was required. Existing tool names, parameters, disabled_tools overrides, and provider behavior remain compatible.

## 8. Security review

Contextual exposure does not bypass ToolBroker, permission, plan mode, sandbox, or parent ceilings. Context artifact reads retain exact session checks. WorkOrder remains separately classified and project-bound.

## 9. Documentation and operations

Updated agent tool-surface and agent architecture documentation with the coding-loop contract and host-derived contextual disclosure rules.

## 10. Unresolved findings (severity: critical/high/medium/low)

None. The stale plan test label `context_compaction` was recorded and substituted with the repository's actual `compaction` target.

## 11. Roadmap disposition

M002 is closed. M003 compact discovery/tool ergonomics and M004 structured verification are now dependency-ready. M005 remains ready from the M001 audit.

## 12. Registry updates

The blocked-work audit found M003 and M004 had all hard/interface dependencies satisfied by M001 and M002. Both are moved to ready in the roadmap, implementation plans, and registry. M005 remains ready. No corrective plan is required.
