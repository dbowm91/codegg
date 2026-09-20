# Coding-Agent Tool Surface Corrective M001 — Surface Authority and Discovery Correctness

Status: implemented

Repository baseline: `99f198293a56a3e190fa83241eeeaaa0e82a3ea6` (production baseline; later planning-only commits do not alter the audited runtime)

Source roadmap:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m001--surface-authority-and-discovery-correctness`

Long-term requirements:

- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/003-planning-process.md#7-corrective-passes`

Related closed work:

- `plans/closure/post-audit-maintainability-surface/002-status.md`
- `plans/closure/tool-surface-upstream-compatibility/007-status.md`
- `plans/closure/tool-surface-upstream-compatibility/008-status.md`
- `plans/closure/long-horizon-work-execution/003-status.md`
- `plans/closure/project-work-orders-task-view/006-status.md`

Applicable ADRs: none. Registration, broker authority, model profiles, and parent ceilings already have canonical owners. Stop if implementation would change those owners.

Primary class: invariant

## 1. Objective

Make the model-facing tool surface truthful and monotonic from registration through invocation.

This milestone must fix four audited correctness defects as one ownership-boundary correction:

1. `tool_search` must observe tools registered after its own construction rather than holding a stale cloned catalog;
2. Curated/Minimal prompt palettes must narrow only the initial advertised set, not erase policy-allowed deferred tools from discovery/callability;
3. category/capability classification used by `ResolvedToolSurface` must agree with the actual tool contract/category instead of depending on a drifting duplicate name map that defaults unknown tools to filesystem mutation;
4. `apply_patch` availability must stop depending on a vendor/model-name substring heuristic and instead use explicit model/profile/policy capability.

The externally visible result is that a policy-allowed coding tool is either immediately advertised or discoverable for a documented reason, never silently lost due to construction order or profile presentation.

## 2. Why this milestone is ready

The required execution owners already exist:

- `ToolRegistry` owns native registration/catalog metadata;
- `ResolvedToolSurface` owns per-turn resolved model-facing capability;
- `ToolBroker` and permission checking own invocation authority;
- `disclosure.rs` owns Core/Deferred/Hidden/profile-specific presentation;
- model profiles own model-specific tool exposure and explicit disabled tools.

No storage migration, provider protocol, scheduler change, or new authorization model is required. The defects are internal ordering/metadata-consistency failures across those established seams.

## 3. Current implementation evidence

At the baseline:

### Stale discovery catalog

`ToolRegistry::with_options()` registers most tools, then constructs:

```text
ToolSearchTool::new(Arc::new(registry.catalog().clone()))
```

`ToolRegistry::register()` later calls `self.catalog.register(&tool)`, but that mutates only the registry's live catalog. The `ToolSearchTool` owns the earlier clone.

Later registrations include:

- `context_read` after `tool_search` in `with_options()`;
- session-scoped `work_order` in `build_session_tool_registry()`;
- session-scoped `goal_get`, `goal_update_progress`, and `goal_request_completion`;
- task replacement/registration in session construction.

The allow-list installed through `set_search_tool_available_tools()` cannot repair missing metadata in the search tool's cloned catalog.

### Destructive palette filtering

`build_tool_definitions()` obtains definitions, then `apply_tool_exposure_filter()` removes names not present in `CURATED_PALETTE` or `MINIMAL_PALETTE`. Only after that does the loop resolve the surface and partition immediate vs deferred tools.

This means a tool omitted from a palette is not merely withheld from the initial prompt; it may never enter the deferred set or `tool_search` available universe. That contradicts the stated `MinimalWithDiscovery` contract.

### Category/capability drift

Individual tools expose `Tool::category()`, but `ResolvedToolSurface` calls `permission::tool_category_for_name()`. The latter is a manually maintained string match and defaults unknown names to `Mutating`.

Representative mismatches to pin in regression tests:

- deterministic eggsact helpers: implementation category `ReadOnly`;
- `context_read`: implementation category `ReadOnly`;
- `work_plan_get`: implementation category `ReadOnly`;
- `work_plan_update_item`: implementation category `SafeMutating`;
- newer Goal/WorkOrder surfaces require explicit semantic disposition rather than accidental fallback.

`Capability::ManageGoals` exists but is not meaningfully projected for Goal/WorkPlan names at the baseline.

### Model-name mutation gate

`filter_tools_for_model()` computes flags from lowercase model IDs and permits `apply_patch` only when the ID contains `gpt`. This predates the richer model-profile and `disabled_tools` system and can remove a canonical native mutation tool from otherwise capable non-GPT models.

## 4. Invariants that must not regress

- Registration, disclosure, discovery, and invocation remain distinct concepts.
- Disclosure/discovery can only narrow visibility; they do not grant authority.
- Hidden/internal tools (`invalid`, program-only adapters) remain undiscoverable.
- Disabled/missing-backend tools remain unadvertised/unavailable rather than discoverable as if callable.
- Parent capability ceilings remain monotonic and child agents cannot widen authority.
- Plan mode remains a separate explicit policy restriction.
- Provider wire aliases remain canonicalized before authority decisions.
- MCP tools remain namespaced and cannot shadow native tools.
- A provider that lacks native defer-loading support may receive all allowed definitions immediately; M001 does not invent provider support.
- Unknown third-party/MCP tools must continue to fail safe from an authority perspective without forcing all native future tools into write authority.
- `ToolBroker` remains invocation authority; no parallel execution router is created.

## 5. Scope

### In scope

- Replace the cloned catalog relationship with a live/shared registry catalog or another construction-safe mechanism that guarantees later registrations are searchable.
- Reorder/refactor model-surface preparation so the complete policy-allowed surface exists before initial-palette projection.
- Preserve a separate full discovery universe and initial-advertisement projection.
- Reconcile category/capability derivation with canonical tool metadata.
- Explicitly classify Goal/WorkPlan/WorkOrder semantics needed for parent ceilings.
- Remove the GPT/vendor-string `apply_patch` gate; express any actual incompatibility through model profile policy/disabled tools.
- Regression tests for late registrations, Curated/Minimal discovery, specialist overrides, plan mode, parent ceilings, hidden/unavailable tools, and representative profiles.
- Documentation updates.

### Explicitly out of scope

- Changing which tools belong in Curated/Minimal beyond what is required for correctness. That belongs to M002.
- Splitting `git`, `lsp`, or `task` schemas. M003.
- Adding `verify` or preview-application tools. M004/M005.
- Removing compatibility aliases such as `codesearch`.
- Provider transport/protocol changes.
- New persistent index/catalog service.
- Permission-level redesign.

## 6. Required production changes

### Core/domain

Establish one authoritative per-tool semantic descriptor available to surface resolution. The implementation may extend existing registry/catalog metadata or `ToolDefinition`-adjacent internal types, but avoid duplicating another static name table.

The descriptor must be able to answer at minimum:

- canonical name;
- `ToolCategory`;
- disclosure class;
- functional-backend state;
- capability requirements needed for parent ceiling checks;
- model-facing schema/description.

If `Tool::category()` remains authoritative, surface construction from a live registry should carry that value forward rather than recomputing it by name. For MCP/provider-only definitions lacking a `Tool` instance, keep a conservative external-tool fallback explicitly separate from native metadata.

Dispose `Capability::ManageGoals` intentionally: map Goal/WorkPlan state management to it where semantically correct, or remove/defer the unused capability only if repository-wide evidence shows no contract depends on it. Do not silently treat Goal state mutation as filesystem mutation.

### Tool catalog/discovery

Prefer shared ownership, e.g. an `Arc<RwLock<ToolCatalog>>` or immutable snapshot rebuilt only after all session registrations are complete. The chosen design must prove:

- every successful `ToolRegistry::register()` appears in discovery metadata unless hidden;
- replacing a same-name tool cannot leave stale metadata;
- search iteration is deterministic;
- there is no second catalog with independent lifecycle.

If shared locking is used, reads must remain bounded and no async lock should be held across model/network execution.

### Surface preparation order

Refactor `build_tool_definitions()` conceptually into:

1. enumerate registered/externally supplied definitions plus canonical metadata;
2. remove hidden/nonfunctional/denied/disabled/plan/parent-ceiling-ineligible tools;
3. form the complete allowed surface and discovery universe;
4. derive initial advertisement from profile exposure mode;
5. apply provider-native deferral where supported;
6. configure `tool_search` with the complete discoverable allowed universe, not only initially advertised names.

Specialist role overrides operate on the complete allowed surface and may make a deferred tool immediate, but never resurrect denied/unavailable tools.

### Model profile gate

Delete the hard-coded `model_id.contains("gpt")` rule for `apply_patch`. If known models truly require disabling the tool, represent that in `ResolvedModelProfile.disabled_tools` or an explicit capability field with tests. Unknown models should not lose the native patch primitive solely because their name is unfamiliar.

### Security and authorization

Ensure `ResolvedToolSurface` capability checks use semantic requirements that match the canonical tool. Add negative tests proving a read-only child can receive/read `context_read` and `work_plan_get` when otherwise permitted but cannot receive `work_plan_update_item`, filesystem edits, shell mutation, or WorkOrder creation beyond its ceiling.

## 7. Ordered work packages

### Work package A — Four-state regression fixtures first

Create focused tests that distinguish:

```text
registered
advertised now
discoverable
callable
```

Include late-registered session tools and hidden/unavailable controls.

Acceptance evidence: tests fail on the baseline for the stale catalog and destructive Curated/Minimal behavior.

### Work package B — Live catalog lifecycle

Remove the cloned search-catalog lifetime bug and make registration/search metadata coherent.

Acceptance evidence:

- register a tool after `tool_search` construction and find it;
- replace/register same canonical name and observe current metadata;
- hidden tools remain absent;
- bounded result cap and deterministic ordering remain.

### Work package C — Full allowed surface before initial palette

Refactor request preparation so Curated/Minimal is an initial-advertisement projection over a retained allowed universe.

Acceptance evidence:

- a Deferred tool omitted from Curated/Minimal is searchable/callable after discovery;
- denied/disabled/plan-mode/parent-ceiling tools remain absent from both advertisement and discovery;
- specialist immediate overrides work after policy narrowing.

### Work package D — Canonical semantic classification

Remove native surface dependence on the drifting name-only category map or generate that map from the same authoritative metadata.

Acceptance evidence:

- representative native tool category equality test iterates the registry and compares surface semantics to `Tool::category()`;
- deterministic/context/WorkPlan/Goal cases are pinned;
- parent-ceiling tests exercise read-only vs state-mutation distinctions.

### Work package E — Explicit model capability policy

Remove vendor substring logic and preserve model-specific compatibility through profile configuration.

Acceptance evidence:

- representative GPT, Claude/Gemini, MiniMax/tool-fragile, local, and unknown model profiles all receive `apply_patch` unless explicitly disabled;
- explicit `disabled_tools=["apply_patch"]` still removes it.

### Work package F — Documentation and stale-list census

Update `architecture/tool.md`, `architecture/agent-tool-surface.md`, model-profile docs, and permission docs. Search for literal static tool lists and either connect them to canonical metadata or document why they are a compatibility/UI list rather than authority.

## 8. Failure, cancellation, restart, and contention semantics

This milestone introduces no long-running execution.

A discovery/catalog failure must fail closed: do not expose an unclassified tool merely to preserve convenience. Registry construction failure should produce explicit diagnostics rather than silently fall back to stale metadata.

Concurrent read access to a shared catalog must not block tool execution for unbounded periods. Registration is construction-time/session-time and must be deterministic. Restart reconstructs the catalog from the same registry/session context; no durable catalog migration exists.

## 9. Compatibility and migration

No database or on-disk migration.

Preserve:

- native tool names and schemas;
- provider wire aliases;
- model-profile `disabled_tools`;
- `tool_search` input/output compatibility where practical;
- stored transcript/run references;
- historical permission records.

Internal additions to tool metadata should be serde/protocol-neutral unless they cross an existing provider boundary. Do not send internal authority metadata to providers unless already part of the tool-definition contract.

## 10. Required tests

### Focused unit tests

- late registration becomes searchable;
- hidden late registration remains hidden;
- catalog replacement/current metadata behavior;
- category equality across all registered native tools;
- Goal/WorkPlan capability classification;
- `apply_patch` profile behavior;
- unknown external/MCP conservative classification.

### Integration tests

Extend/add tool-surface tests covering:

- Full, Curated, MinimalWithDiscovery;
- provider with defer-loading support vs without it;
- ordinary, research, security-review, verifier roles;
- plan mode;
- functional vs disabled LSP/security backends;
- task with vs without functional spawner;
- session registry containing `context_read`, WorkPlan, Goal, WorkOrder;
- child parent-ceiling narrowing.

### Restart/recovery tests

Construct equivalent session registry twice and assert deterministic discovery/surface fingerprint/semantic metadata for stable configuration.

### Contention/cancellation tests

If a shared lock is introduced for the catalog, add a focused concurrent register/read test or prove registration is finalized before concurrent reads. No generic stress harness.

### Security/negative tests

- denied tool never returned by search;
- hidden internal tools never returned;
- parent ceiling cannot be bypassed by discovery;
- model profile disable cannot be bypassed by discovery;
- plan mode cannot discover/call mutation;
- late registration cannot shadow canonical names with incompatible metadata silently.

## 11. Required verification commands

Use current exact test target names at implementation time. Expected minimum:

```bash
cargo test -p codegg --lib tool::catalog
cargo test -p codegg --lib tool::tool_search
cargo test -p codegg --lib agent::tool_surface
cargo test -p codegg --lib agent::tool_inspect
cargo test --test tool_surface_minimization
cargo test --test tool_registry
cargo test --test tool_execution
cargo test --test work_orders_m006_agent_tool
cargo test --test work_plan_projection_arbiter
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Do not add a new CI lane.

## 12. Documentation updates

- `architecture/tool.md`: live catalog lifecycle; four-state registration/advertisement/discovery/invocation contract.
- `architecture/agent-tool-surface.md`: corrected resolution order and semantic metadata owner.
- `architecture/permission.md`: category/authority source of truth.
- `architecture/agent.md` or model-profile docs: explicit model capability/disabled-tool behavior.
- planning closure must list any static tool lists intentionally retained and why they are non-authoritative.

## 13. Acceptance criteria

M001 closes only when:

1. a tool registered after `tool_search` construction is discoverable when policy allows it;
2. Curated/Minimal profiles can discover deferred policy-allowed tools that are not initially advertised;
3. hidden/denied/disabled/plan/parent-ceiling-ineligible tools remain undiscoverable and uncallable;
4. surface category/capability semantics agree with canonical native tool semantics, including deterministic/context/WorkPlan/Goal cases;
5. `apply_patch` no longer depends on model vendor/name substring matching;
6. no duplicate execution, permission, scheduler, or catalog owner is introduced;
7. focused and broad verification passes.

## 14. Stop conditions

Stop and report rather than improvise if:

- fixing classification requires a provider wire-protocol break;
- one of the audited mismatches is intentional authority behavior not documented elsewhere;
- a model family demonstrably requires a fundamentally different mutation schema rather than profile disable/aliasing;
- the current HEAD has already corrected one finding—record evidence and narrow the plan rather than reintroducing a second mechanism;
- a shared catalog would require persistent indexing or background synchronization.

## 15. Closure evidence required

The closure record must include:

- before/after resolution pipeline;
- proof of the baseline failures and new regression tests;
- live-catalog ownership and concurrency rationale;
- complete audited native category/capability reconciliation table or generated invariant evidence;
- profile matrix for `apply_patch`;
- Full/Curated/Minimal discovery matrix;
- specialist/plan/parent-ceiling negative evidence;
- exact commands/results;
- severity-classified residual findings;
- registry unblock audit for M002 and M005.

## 16. Handoff notes

Do not solve this by merely adding `work_order`, `goal_*`, `context_read`, and other missing names to more static lists. The defect is duplicated lifecycle/authority metadata. The implementation should reduce the number of independent lists and make future tool registration correct by construction.
