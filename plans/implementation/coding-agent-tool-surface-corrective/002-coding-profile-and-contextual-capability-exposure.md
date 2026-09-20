# Coding-Agent Tool Surface Corrective M002 — Coding Profile and Contextual Capability Exposure

Status: blocked

Repository baseline: `99f198293a56a3e190fa83241eeeaaa0e82a3ea6` (production baseline)

Source roadmap:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#M002--coding-profile-and-contextual-capability-exposure`

Hard dependency:

- M001 `plans/implementation/coding-agent-tool-surface-corrective/001-surface-authority-and-discovery-correctness.md` must close first.

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs: none.

Primary class: capability

## 1. Objective

Define and implement a coherent coding-agent profile surface after M001 establishes a truthful discovery/authority pipeline.

Ordinary coding profiles must retain the complete native coding loop:

```text
inspect -> edit/create -> verify -> inspect context/results -> continue
```

while long-horizon/project scheduling tools are exposed only when bound runtime context makes them relevant.

The milestone must specifically prevent profile minimization from pushing normal work onto less-restorable generic shell paths merely because native tools were omitted from the initial palette.

## 2. Why this milestone is blocked

M002 changes policy/presentation, not authority. It must consume the corrected M001 distinction between the full allowed discovery universe and the initial advertised palette. Implementing M002 before M001 would require adding more static-name exceptions to a defective pipeline.

After M001 closes, all required execution owners are already stable: native edit tools, supervised test runner, context artifact store, Goal/WorkPlan/WorkOrder services, model profiles, broker/permissions, and scheduler.

## 3. Current implementation evidence

At the audit baseline:

- FrontierReasoning, FrontierExecutor, LongContextPlanner, and Reviewer profiles default to `Curated`.
- FastExecutor, ToolFragile, LocalStrict, and Summarizer default to `MinimalWithDiscovery`.
- `CURATED_PALETTE` omits `test`, `write`, `lsp`, `context_read`, and plan controls.
- `MINIMAL_PALETTE` is narrower and can rely heavily on `edit` + `bash`.
- `test` is a stronger coding primitive than generic shell testing: it uses scheduler-owned execution, bounded reports, scope selection, previous-failure support, timeout/stall handling, and structured execution provenance.
- `context_read` is required to expand `ctx://` artifacts produced by compaction/tool-output projection. A profile that can receive a handle but cannot use the expansion tool has a broken recovery loop.
- native `write`/`apply_patch` participate in the restorable edit-checkpoint surface; arbitrary Bash filesystem mutation does not.
- session/project construction can register WorkPlan, Goal, and WorkOrder tools. These are useful when bound state exists but unnecessary prompt weight for unrelated coding turns.

## 4. Invariants that must not regress

- Profile exposure cannot widen invocation authority.
- `disabled_tools`, plan mode, backend availability, agent denies, parent ceilings, sandbox, and permission mode remain authoritative.
- Initial-prompt minimization may defer tools but must not make a normal coding loop impossible.
- A `ctx://` handle shown to the model implies `context_read` is callable or the handle must not be emitted as an actionable recovery mechanism.
- Native restorable mutation is preferred over shell mutation for ordinary file creation/editing.
- The test tool remains scheduler-owned; M002 does not add another test executor.
- Goal/WorkPlan/WorkOrder exposure is based on host-bound state, not semantic guessing from user prose.
- WorkOrder remains distinct from live child delegation through `task`.

## 5. Scope

### In scope

- Reconcile `CORE_PALETTE`, `CURATED_PALETTE`, and `MINIMAL_PALETTE` against actual coding-loop requirements after M001.
- Make `test` immediately available in normal coding profiles where its scheduler backend is functional, or prove an equivalent bounded contextual rule.
- Make `context_read` immediately available whenever context artifact handles may be emitted for the current session.
- Ensure capable profiles retain a native file-creation path (`write` and/or `apply_patch`) without shell fallback.
- Keep `lsp` available according to functional backend/profile value; use deferral rather than deletion where initial schema cost is too high.
- Add host-driven contextual disclosure for WorkPlan/Goal/WorkOrder tools when relevant bound state exists.
- Preserve `tool_search` for deferred specialist capabilities.
- Profile matrix tests and documentation.

### Explicitly out of scope

- New tool discovery protocol. M003.
- Splitting large schemas. M003.
- New verification tool. M004.
- LSP preview mutation. M005.
- Rewriting model-profile inference or introducing an ML tool router.
- Automatically creating Goal/WorkPlan/WorkOrder state.
- Prompt heuristics that guess long-horizon state from natural language.

## 6. Required production changes

### Coding-loop palette contract

Define a small set of semantic requirements rather than relying only on literal tool names:

1. file read/inspection;
2. repository search/listing;
3. precise edit;
4. file creation/patch;
5. controlled command/build path;
6. supervised test verification;
7. artifact/context recovery;
8. tool discovery.

Map available canonical tools into those requirements. The simplest implementation may still use canonical names, but the tests must assert semantic completeness across profiles.

Recommended ordinary Curated immediate set should include, when functional:

- `read`, `list`, `grep`, `glob`, `repo_search`;
- `edit`, `write` and/or `apply_patch`;
- `bash`, `test`, `git`;
- `context_read` when registered;
- `tool_search`, `question`, `skill`;
- todo tools according to task-state policy.

Minimal profiles may keep fewer inspection/edit variants but must preserve one route for each coding-loop semantic requirement. If `apply_patch` is too schema-fragile for a known profile, `write` + `edit` must still cover create/update without Bash.

### Contextual long-horizon disclosure

Introduce a small host-derived context descriptor during turn construction, for example booleans such as:

- active Goal for session;
- active WorkPlan for session;
- durable project WorkOrder capability bound and enabled.

Use it only for initial disclosure. The tools must already be registered and policy-allowed through M001.

Recommended behavior:

- active Goal -> `goal_get`, `goal_update_progress`, `goal_request_completion` immediate;
- active WorkPlan -> `work_plan_get`, `work_plan_update_item` immediate;
- WorkOrder bound + explicitly enabled project policy -> keep `work_order` deferred by default, but make it easy to discover; immediate only in task-planning/work-order contexts if an existing host mode provides that signal.

Do not query storage repeatedly per tool. Resolve bounded state once per turn/session preparation using existing stores.

### Context handle contract

Audit the code path that emits `ctx://` handles. If `context_read` backend is unavailable, output must be explicit that expansion is unavailable rather than instructing the model to call a missing tool.

### LSP

Do not force the full current `lsp` schema into Minimal profiles merely because LSP is useful. M002 may keep it deferred/discoverable after M001 if schema size is material. Curated/Reviewer profiles can include it immediately when functional. M003 owns later schema decomposition.

## 7. Ordered work packages

### Work package A — Profile capability matrix

Create a table/test fixture for Default, FrontierReasoning, FrontierExecutor, LongContextPlanner, FastExecutor, ToolFragile, LocalStrict, Reviewer, and Summarizer. For each, assert required semantic coding-loop capabilities and intended immediate/deferred state.

Acceptance evidence: baseline gaps for `test`, `context_read`, or native file creation are explicit.

### Work package B — Coding palette correction

Adjust palettes/profile exposure to satisfy the semantic matrix with the smallest initial schema set.

Acceptance evidence: no normal coding profile requires Bash solely to create a new source/config file or run ordinary project tests when canonical native/supervised tools are functional.

### Work package C — Context recovery contract

Bind `context_read` exposure to artifact-handle availability/registration and test compaction/tool-output recovery trajectories.

Acceptance evidence: a model-visible actionable `ctx://` handle can always be expanded in the same session, subject to normal session/authority checks.

### Work package D — Contextual Goal/WorkPlan disclosure

Resolve active Goal/WorkPlan state once and inject their tool definitions immediately only when relevant.

Acceptance evidence: unrelated short coding turn has no Goal/WorkPlan schema bloat; a session with an active plan receives the exact bounded plan tools without tool-search guessing.

### Work package E — WorkOrder disposition

Keep WorkOrder semantically separate from TaskTool. Verify project-bound functional state and choose deferred-vs-contextual-immediate behavior based on existing Task/workspace mode signals.

Acceptance evidence: ordinary edit turn is not bloated; project scheduling/task composition context can discover/use WorkOrder without requiring raw catalog knowledge.

### Work package F — Documentation/profile regression

Update architecture/model-profile docs and add snapshot/matrix tests resilient to future tool additions.

## 8. Failure, cancellation, restart, and contention semantics

No new execution runtime is added.

Context-state lookup failure must fail closed toward smaller disclosure: do not fabricate an active Goal/WorkPlan. Existing active state remains authoritative after restart through its canonical store.

If context artifact storage is disabled/unavailable, do not advertise a nonfunctional `context_read`. Do not suppress artifact content into an unusable handle.

Concurrent Goal/WorkPlan changes use existing revision/store semantics. Tool disclosure is a per-turn snapshot; a plan created after request preparation becomes visible on the next turn.

## 9. Compatibility and migration

No storage/protocol migration.

Existing model-profile `disabled_tools` overrides remain valid. Existing custom profile behavior should be preserved unless it depended on an accidental omission; document any newly visible immediate tool.

Do not remove tool names or parameters.

## 10. Required tests

### Focused unit tests

- semantic capability matrix by built-in profile;
- contextual disclosure booleans/state resolver;
- context_read availability/handle contract;
- active vs inactive Goal/WorkPlan definition sets;
- WorkOrder functional/deferred disposition.

### Integration tests

- create new file + edit + test loop under representative Frontier and ToolFragile profiles;
- compaction/tool-output artifact handle -> context_read recovery;
- active WorkPlan session turn receives plan tools and updates revision-checked item;
- inactive session does not receive plan tool schema initially;
- explicit disabled tool remains unavailable even when contextually relevant.

### Restart/recovery tests

- active Goal/WorkPlan after store restart still causes the same contextual exposure;
- session with artifact handle/store reconstruction remains recoverable where supported.

### Security/negative tests

- read-only child does not gain mutation due to contextual exposure;
- project without WorkOrder capability cannot discover/call it as functional;
- plan mode restrictions remain intact;
- context_read cross-session denial remains.

## 11. Required verification commands

```bash
cargo test -p codegg --lib agent::policy
cargo test -p codegg --lib agent::request_preparation
cargo test --test tool_surface_minimization
cargo test --test work_plan_projection_arbiter
cargo test --test work_orders_m006_agent_tool
cargo test --test context_compaction
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Adjust only for actual current test target names. No new CI lane.

## 12. Documentation updates

- `architecture/agent-tool-surface.md`
- `architecture/agent.md`
- `architecture/tool.md`
- `architecture/compaction.md` or context docs where handle recovery is described
- long-horizon/WorkOrder architecture docs only if disclosure behavior is documented there

## 13. Acceptance criteria

M002 closes when every supported coding-oriented profile has a complete native/supervised coding loop; context handles are actionable; active Goal/WorkPlan state receives bounded contextual tools; unrelated turns avoid that schema; WorkOrder remains distinct and correctly scoped; and all policy/parent/sandbox/permission restrictions remain monotonic.

## 14. Stop conditions

Stop if:

- M001 is not strictly closed;
- satisfying a profile requires changing scheduler, permission, or sandbox ownership;
- contextual disclosure requires a new durable state store;
- a provider schema limit makes even the corrected minimal semantic loop impossible without a provider-specific protocol decision;
- current evidence shows a named profile cannot reliably call a proposed core tool and no existing adapter/profile disable can represent that incompatibility.

## 15. Closure evidence required

Include:

- final profile capability matrix;
- initial vs deferred tool sets for representative profiles;
- context-handle recovery trajectory;
- active/inactive Goal/WorkPlan comparison;
- WorkOrder disposition rationale;
- child/plan/disabled negative tests;
- exact verification results;
- M003/M004 unblock audit.

## 16. Handoff notes

Prefer semantic completeness over literal palette symmetry. Minimal does not mean “fewest names at any cost”; it means the smallest surface that can still complete normal coding work without escaping into generic shell mutation or unrecoverable context.
