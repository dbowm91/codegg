# Coding-Agent Tool Surface Corrective Roadmap

Status: active

Repository audit baseline: `99f198293a56a3e190fa83241eeeaaa0e82a3ea6`

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/003-planning-process.md#7-corrective-passes`

Related closed work:

- `plans/subsystems/post-audit-maintainability-surface-corrective-addendum.md`
- `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md`
- `plans/subsystems/long-horizon-work-execution-roadmap.md`
- `plans/subsystems/project-work-orders-task-view-roadmap.md`
- `plans/subsystems/tool-programs-roadmap.md`

Related ADRs:

- None required initially. This campaign corrects and projects existing tool, permission, model-profile, scheduler, LSP, and edit-checkpoint ownership. If implementation would change those owners rather than consume them, stop and register the appropriate ADR.

## 1. Purpose and ownership boundary

This roadmap owns the model-facing coding-agent tool surface: registration-to-discovery correctness, prompt disclosure, tool classification/capability metadata, model-profile palette behavior, tool-selection ergonomics, and small model-facing facades over already-canonical execution services.

It does not own filesystem execution, scheduler admission, Git mutation semantics, LSP process lifecycle, WorkPlan/Goal/WorkOrder persistence, approval policy, or sandbox policy. Those remain with their established subsystems. This work may expose or adapt those capabilities to agents but must not create parallel owners.

The corrective trigger is a September 20, 2026 audit of the current coding-agent tool surface. The audit found strong underlying capabilities but several inconsistencies between registration, disclosure, discovery, capability classification, and profile filtering that can make callable tools invisible or assign them incorrect authority.

## 2. Work classification

### Invariants

- Progressive disclosure narrows the initial prompt, not the policy-allowed discovery universe.
- A registered policy-allowed deferred tool remains discoverable unless explicitly hidden or unavailable.
- Tool authority/capability classification agrees with the canonical tool contract and cannot silently default read-only tools to filesystem-write authority.
- Parent/subagent authority ceilings remain monotonic.
- Model identity/vendor strings do not silently grant or remove filesystem mutation authority.
- Context artifact handles remain recoverable by a model that receives them.
- Native restorable edit primitives remain preferred over opaque shell mutation for normal coding work.

### Capabilities

- Ordinary coding profiles retain a complete edit/test/context-recovery loop.
- Goal/WorkPlan/WorkOrder surfaces become available when their session/project context makes them relevant without permanently bloating every prompt.
- Models can discover tools compactly and request a full schema only when needed.
- Routine compile/lint/type/format verification can use one bounded structured facade over existing command-intent/scheduler machinery.
- LSP preview artifacts can be applied through a checked native mutation path without reconstructing edits from prose.

### Infrastructure

- Live/shared tool catalog identity for post-construction registration.
- One authoritative tool semantic/classification source consumed by disclosure, permission/capability, and discovery.
- Compact tool descriptor lookup.
- Thin facades over existing managed command and checked edit machinery.

### Polish

- Reduce large-schema selection entropy for `git`, `lsp`, and `task`.
- Remove stale model-name heuristics and duplicated classification lists.
- Improve architecture documentation and regression coverage.

## 3. Non-goals

- Replacing `ToolRegistry`, `ToolBroker`, model profiles, provider adapters, scheduler, or permission system.
- Creating a second tool discovery/indexing system.
- Adding a generic workflow engine or generic command framework.
- Replacing Git/LSP/task implementations merely to change their model-facing shape.
- Removing compatibility aliases without a consumer census.
- Creating new CI lanes, benchmark gates, or live-provider/network CI.
- Broadening child-agent authority.
- Making LSP previews mutate files implicitly.
- Auto-applying model-generated edits without existing permission/checkpoint enforcement.

## 4. Current state and audit evidence

At the baseline:

- `ToolRegistry::register()` updates the live registry catalog, but `tool_search` is constructed inside `with_options()` from `Arc::new(registry.catalog().clone())`. Session/factory registrations performed later do not enter the search tool's catalog snapshot.
- `build_session_tool_registry()` adds session-scoped `work_order` and `goal_*` tools after the base registry; `context_read` is also registered after `tool_search` inside `with_options()`.
- `apply_tool_exposure_filter()` applies Curated/Minimal palettes before the complete discovery universe is retained. Tools excluded from the initial palette can therefore disappear rather than remain deferred/discoverable.
- `filter_tools_for_model()` still gates `apply_patch` using a model-name substring heuristic. This conflicts with the newer model-profile policy system.
- `ResolvedToolSurface` obtains categories through `permission::tool_category_for_name()`, a second name-based map. Unknown tools default to `Mutating`; newer read-only/safe-mutating tools such as deterministic validators, `context_read`, and WorkPlan reads are not fully represented.
- Curated/Minimal palettes omit high-value coding-loop capabilities including `test`, `context_read`, and in some profiles safe file-creation paths.
- `lsp`, `git`, and `task` are broad multiplexers with large operation enums/schemas. This is prompt-efficient at the tool-name level but raises argument/action selection entropy for smaller/tool-fragile models.
- The broker already supports structured contracts/output schemas. LSP already produces preview artifacts **and** the closed Architecture Convergence M007 path already provides daemon-owned checked preview application (`CoreRequest::LspPreviewApply` -> `src/lsp/mutation.rs`) with digest/hash revalidation and edit checkpoints; what is absent is a model-facing tool adapter over that existing authority. Command-intent already classifies build/lint/format/test families. Improvements should compose these installed owners rather than invent replacements.

## 5. Target architecture

The intended flow is:

```text
registered tools + runtime backend state
        |
        v
canonical tool semantics / authority metadata
        |
        v
policy-allowed callable surface
        |-----------------------------+
        |                             |
        v                             v
discovery universe              initial prompt palette
(full allowed deferred set)     (Core/Curated/Minimal/contextual)
        |                             |
        +---------- tool_search ------+
```

The catalog used by discovery is live for the registry lifetime. Initial disclosure is a projection over the complete allowed surface, not a destructive filter that erases deferred capability.

Tool-selection ergonomics should use compact descriptors first and full schemas second. Large capability families may expose smaller semantic facades, but canonical implementation owners remain unchanged.

## 6. Dependency graph

```text
M001 Surface authority and discovery correctness
  |
  +--> M002 Coding profile and contextual capability exposure
  |      |
  |      +--> M003 Compact discovery and multiplexed-tool ergonomics
  |      |
  |      +--> M004 Structured verification facade
  |
  `--> M005 Checked LSP preview application
```

Dependency classes:

- M001 -> M002: hard. Palette improvements must build on a correct discovery/authority model.
- M002 -> M003: interface. Ergonomic reduction must know the intended core/contextual surface.
- M002 -> M004: interface. Verification exposure should follow the corrected coding profile contract.
- M001 -> M005: hard. Preview application must enter the canonical authority/category/checkpoint model correctly.

## 7. Milestones

### M001 — Surface authority and discovery correctness

Class: invariant.

Implementation: `plans/implementation/coding-agent-tool-surface-corrective/001-surface-authority-and-discovery-correctness.md`

Objective: correct the registration/discovery lifecycle, progressive-disclosure ordering, tool semantic classification, and legacy model-name mutation gate so callable capability is neither accidentally hidden nor assigned incorrect authority.

Exit conditions:

- late registry registrations are represented in `tool_search`;
- Curated/Minimal filtering does not delete policy-allowed deferred discovery;
- category/capability metadata is derived from one canonical semantic source or proven synchronized from generated metadata;
- `apply_patch` availability is profile/policy-driven, not vendor-string-driven;
- parent ceilings and permission behavior remain monotonic and covered by regression tests.

### M002 — Coding profile and contextual capability exposure

Class: capability.

Implementation: `plans/implementation/coding-agent-tool-surface-corrective/002-coding-profile-and-contextual-capability-exposure.md`

Objective: ensure ordinary coding profiles retain the complete inspect/edit/test/context-recovery loop while injecting Goal/WorkPlan/WorkOrder surfaces only when their bound context is relevant.

Exit conditions:

- `test` and `context_read` are usable by normal coding profiles when functional;
- safe file creation/edit paths do not depend on generic shell fallback;
- long-horizon/session tools are exposed contextually and remain discoverable according to policy;
- no profile receives authority beyond its resolved ceiling.

### M003 — Compact discovery and multiplexed-tool ergonomics

Class: polish / capability.

Implementation: `plans/implementation/coding-agent-tool-surface-corrective/003-compact-discovery-and-multiplexed-tool-ergonomics.md`

Objective: reduce tool-selection/schema entropy through compact search descriptors, explicit schema description, and bounded semantic facades for the largest multiplexed tools without duplicating execution owners.

Exit conditions:

- broad `tool_search` no longer injects full large schemas for every match;
- one selected tool can be expanded deterministically to its complete current schema;
- `git`, `lsp`, and `task` expose smaller model-facing semantic surfaces where evidence shows value, delegating canonical implementations;
- compatibility names remain supported as required.

### M004 — Structured verification facade

Class: capability.

Implementation: `plans/implementation/coding-agent-tool-surface-corrective/004-structured-verification-facade.md`

Objective: expose routine build/lint/typecheck/format-check verification as a bounded structured tool over existing command-intent, scheduler, managed-process, and diagnostics machinery.

### M005 — Checked LSP preview application

Class: capability / invariant.

Implementation: `plans/implementation/coding-agent-tool-surface-corrective/005-checked-lsp-preview-application.md`

Objective: add a narrow model-facing adapter for applying a previously-created LSP preview by reusing the already-closed daemon-owned checked mutation service; the model supplies preview identity only, while host-owned preview metadata, stale/path validation, workspace locking, rollback/checkpointing, and LSP synchronization remain canonical.

## 8. Cross-cutting requirements

### Storage and migration

M001-M004 should require no storage migration. M005 may extend ephemeral preview metadata/registry contracts but must not create a second durable edit-history store. If persistence is required for preview identity across turns/restarts, stop and register a storage decision rather than improvising.

### Protocol and compatibility

Provider wire aliases, MCP namespacing, stored transcript/tool names, model-profile config, and existing tool schemas remain backward-compatible unless a milestone explicitly supplies a compatibility adapter. New semantic facades may be additive.

### Security and authorization

All disclosure/discovery changes are authority-neutral: visibility cannot widen invocation authority. Parent ceilings, permission modes, sandbox profiles, workspace roots, child Git policy, and broker caller policy remain authoritative.

### Concurrency, cancellation, restart

Catalog updates must be deterministic and safe under registry construction/use. No background discovery thread is authorized. Verification continues through scheduler-owned process execution. Preview application must reject stale artifacts and conflicting workspace state rather than best-effort merging.

### Observability and audit

Reuse existing tool provenance, run store, audit events, scheduler results, and edit checkpoints. Do not add parallel logging/audit streams.

### Performance and resource use

Keep initial model tool definitions bounded. Tool discovery should reduce, not move, schema bloat. No persistent repository index or background model router is introduced.

## 9. Verification strategy

Use focused tool-surface tests that assert four distinct properties: registered, initially advertised, discoverable, and callable. Add profile matrices across Frontier/Curated, Minimal/tool-fragile/local, plan mode, specialist agents, functional/unavailable backends, and child capability ceilings.

Regression fixtures must include session-late tools (`context_read`, `goal_*`, `work_order`) and representative deterministic tools.

For M003-M005, add deterministic fake/local fixtures rather than live providers.

Broad closure posture remains:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

No new CI lane is authorized.

## 10. Risks and decision points

- A truly single canonical classification source may require extending `ToolDefinition` or registry metadata. This is acceptable if internal/additive; a provider wire-protocol change is not.
- Some providers do not support native deferred loading. They may still receive the complete allowed schema set; M001 must not claim protocol-level deferral where unsupported.
- Splitting multiplexed tools can increase tool count. M003 must measure prompt/schema cost and prefer semantic grouping over one-tool-per-operation explosion.
- Contextual Goal/WorkPlan/WorkOrder exposure must be driven by bound runtime state, not guessed from user prose.
- M005 must never treat an LSP preview as authorization to mutate. It is evidence/input to a later checked mutation.

## 11. Completion definition

This campaign closes when registration, disclosure, discovery, category/capability authority, and profile exposure are internally consistent; ordinary coding agents retain a complete native coding loop; tool discovery remains compact; routine verification has a structured canonical facade; and LSP preview edits can be applied through checked native mutation without introducing a new execution or persistence owner.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/coding-agent-tool-surface-corrective/001-surface-authority-and-discovery-correctness.md` | `plans/closure/coding-agent-tool-surface-corrective/001-status.md` | — |
| M002 | ready | `plans/implementation/coding-agent-tool-surface-corrective/002-coding-profile-and-contextual-capability-exposure.md` | — | — |
| M003 | blocked | `plans/implementation/coding-agent-tool-surface-corrective/003-compact-discovery-and-multiplexed-tool-ergonomics.md` | — | M001, M002 |
| M004 | blocked | `plans/implementation/coding-agent-tool-surface-corrective/004-structured-verification-facade.md` | — | M002 |
| M005 | ready | `plans/implementation/coding-agent-tool-surface-corrective/005-checked-lsp-preview-application.md` | — | — |
