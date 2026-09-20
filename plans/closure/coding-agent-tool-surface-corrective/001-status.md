# Coding-Agent Tool Surface Corrective M001 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/coding-agent-tool-surface-corrective/001-surface-authority-and-discovery-correctness.md`
Source subsystem roadmap: `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m001--surface-authority-and-discovery-correctness`
Repository baseline reviewed: `fd63425`
Implementation commits: `fd63425` — correct live discovery authority, deferred palette projection, native category metadata, state capabilities, and model-independent patch exposure

## 1. Executive finding

M001 is complete and strictly closed. The model-facing surface now retains a live policy-allowed discovery universe, projects Curated/Minimal palettes as initial deferral rather than destructive filtering, resolves native categories from registered Tool instances, separates Goal/WorkPlan/WorkOrder state authority from filesystem write authority, and no longer gates apply_patch by vendor/model-name substring.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Late registrations are searchable | Shared ToolCatalog clone regression covers late registration and same-name replacement | Pass |
| Palette omission preserves discovery | request-preparation palette projection regression; deferred names remain in tool_search allow-list | Pass |
| Hidden/denied/disabled/plan/ceiling controls remain monotonic | Existing tool-surface minimization and resolved-surface tests plus quick verification | Pass |
| Native category/capability semantics are authoritative | Registry category parity test; explicit Goal/WorkPlan/WorkOrder capability test | Pass |
| apply_patch is profile/policy-driven | Local/unknown model regression; explicit disabled_tools path remains in policy | Pass |
| No duplicate execution or catalog owner | One shared ToolCatalog lifecycle; broker and permission owners unchanged | Pass |

## 3. Production implementation evidence

- ToolCatalog now uses shared read/write state; cloning a catalog shares live registration metadata, with deterministic name/tie ordering and replacement-safe deferred state.
- Request preparation applies the initial palette after policy/callability filtering and marks omitted allowed definitions deferred. Search receives immediate plus deferred allowed names.
- ResolvedToolSurface accepts native category metadata and uses the legacy name map only for definitions without native metadata, such as external/MCP tools.
- Goal and WorkPlan tools report read-only or safe-mutating categories; WorkOrder is represented by a distinct ManageWorkOrders capability.
- filter_tools_for_model no longer derives mutation availability from model vendor strings.

## 4. Verification executed (local)

- `cargo check -p codegg --lib` — passed.
- `cargo test -p codegg --lib tool_inspect` — 16 passed.
- focused catalog, native-category, and palette regressions — passed.
- `cargo test --test tool_surface_minimization -- --test-threads=1` — 13 passed.
- `cargo test --test tool_registry -- --test-threads=1` — 12 passed.
- `cargo test --test tool_execution -- --test-threads=1` — 55 passed.
- `python3 scripts/check_execution_ownership.py` — passed.
- `python3 scripts/check_scheduler_bypass.py` — passed.
- `scripts/verify.sh quick` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `git diff --check` — passed.

## 5. Invariant review

Registration, advertisement, discovery, and invocation remain distinct. Discovery never grants authority. Hidden tools remain absent, parent ceilings remain monotonic, plan mode remains restrictive, and ToolBroker remains the only production invocation boundary.

## 6. Failure and recovery review

Catalog poisoning is recovered as an explicit lock-poison recovery of the shared in-process state; no stale independent catalog can survive a registration update. No background discovery process, durable catalog, or long-running execution was introduced.

## 7. Migration and compatibility review

No storage, protocol, provider-wire, transcript, or model-profile migration was required. Existing names and schemas remain intact; catalog metadata changes are internal.

## 8. Security review

Native categories are less permissive than the prior unknown-name fallback for state tools, and WorkOrder authority is distinct from filesystem write. Denied, hidden, disabled, plan-mode, and parent-ceiling restrictions remain applied before discovery.

## 9. Documentation and operations

Updated `architecture/agent-tool-surface.md`, `architecture/tool.md`, and `architecture/permission.md` to document live catalog ownership, semantic category provenance, four-state surface behavior, and state capabilities.

## 10. Unresolved findings (severity: critical/high/medium/low)

None.

## 11. Roadmap disposition

M001 is closed. Its interface is stable enough for M002 profile/contextual exposure and M005 checked LSP preview adapter work. M003 and M004 remain blocked on M002 as planned.

## 12. Registry updates

The dependency audit found two downstream plans fully unblocked: M002 and M005. Both are moved to ready in the roadmap, implementation status lines, and registry. M003 and M004 remain blocked on M002. No corrective pass is required.
