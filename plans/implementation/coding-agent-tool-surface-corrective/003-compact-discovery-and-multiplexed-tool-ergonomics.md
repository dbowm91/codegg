# Coding-Agent Tool Surface Corrective M003 — Compact Discovery and Multiplexed-Tool Ergonomics

Status: active

Repository baseline: `99f198293a56a3e190fa83241eeeaaa0e82a3ea6` (production baseline)

Source roadmap:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md#m003--compact-discovery-and-multiplexed-tool-ergonomics`

Dependencies:

- M001 strict closure is required.
- M002 strict closure is required so the intended core/contextual palette is stable before further schema reduction.

Long-term requirements:

- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs: none unless implementation changes canonical Git/LSP/delegation ownership.

Primary class: polish / capability

## 1. Objective

Reduce tool-selection and argument-schema entropy for coding agents, especially ToolFragile/LocalStrict profiles, without duplicating execution implementations.

This milestone has two bounded outcomes:

1. make discovery two-stage so a broad `tool_search` returns compact descriptors rather than embedding every matching full JSON schema;
2. provide smaller semantic model-facing entrypoints for the largest multiplexed tool families (`git`, `lsp`, `task`) only where they demonstrably reduce schema/action ambiguity, with each facade delegating to the existing canonical service.

The milestone is about model ergonomics, not capability growth.

## 2. Why this milestone is blocked

M001 must first guarantee a correct discoverable universe and canonical semantic metadata. M002 must first settle which capabilities are core/contextual/deferred. Otherwise M003 risks optimizing a transient or incorrect surface.

No new backend is needed once those dependencies close.

## 3. Current implementation evidence

### Tool search

`tool_search` currently returns up to ten matches containing:

- canonical/name;
- description;
- full `parameters` JSON schema;
- category/risk/disclosure metadata.

This works for small tools but a broad search matching `lsp`, `git`, or `task` can inject a large schema payload into context merely to choose among candidates.

### LSP

The current `lsp` tool multiplexes roughly three dozen operations including:

- definitions/references/hover/symbols/diagnostics;
- completion/signature/semantic tokens;
- rename/format/source/code-action previews;
- semantic/security context;
- call/type hierarchy;
- hunk context;
- higher-level repair/review/impact/test/interface/cross-file workflows.

The implementation is rich and should remain canonical. The issue is one large action enum plus many parameters relevant only to subsets of operations.

### Git

`git` combines structured read operations, typed mutation operations, operation-state/recovery, branch/stash/remotes/config/reset/clean/network actions, and compatibility raw subcommands.

Again, canonical execution ownership is good; model-facing schema breadth is the concern.

### Task

`task` combines ordinary child spawn/status/message/wait/cancel with groups/fan-out and convergence producer/verifier/control semantics. Many coding turns only need spawn + wait/status/control.

### Existing adapters

Hidden program-only `git_read` and `lsp_read` already show that narrower operation-scoped adapters can delegate canonical owners without creating a second backend. Provider alias machinery also already supports stable canonical/wire mapping.

## 4. Invariants that must not regress

- `git`, LSP services, TaskTool/run-control/group/convergence services remain canonical execution owners.
- New facades cannot bypass `ToolBroker`, permission policy, parent ceilings, child Git policy, scheduler, or sandbox.
- Compatibility tool names remain callable according to existing policy unless separately deprecated with evidence.
- Discovery remains bounded and monotonic.
- Hidden/program-only adapters remain hidden unless intentionally replaced by an explicitly model-facing facade with reviewed authority.
- Tool count reduction is not pursued by making one schema more polymorphic/ambiguous.
- Tool count expansion is not pursued to one-tool-per-operation extremes.
- No semantic routing model or background classifier is introduced.

## 5. Scope

### In scope

- Compact search result schema that omits full parameters by default.
- A new read-only schema-description operation/tool, e.g. `tool_describe`, or equivalent exact-name expansion through `tool_search`.
- Deterministic lookup of one selected tool's full current description/schema/metadata.
- Measure and disposition `git`, `lsp`, and `task` model-facing schemas.
- Add thin semantic facades where useful, likely grouped by authority/task family rather than individual operations.
- Model-profile/disclosure updates for new facades.
- Compatibility alias tests.
- Prompt/schema-size and selection correctness fixtures using deterministic fake providers or direct schema inspection.

### Explicitly out of scope

- Rewriting Git, LSP, task scheduling, convergence, or run-control implementations.
- Removing current multiplexed tools in the first pass.
- Provider protocol changes.
- ML-based tool routing.
- Persistent vector/BM25 indexing beyond the existing catalog.
- New permissions or authorization roles.
- General plugin/MCP schema rewriting.

## 6. Required production changes

### Two-stage discovery

Change the selection path so broad search returns compact descriptors such as:

```json
{
  "name": "lsp_context",
  "purpose": "Gather semantic code context...",
  "category": "read_only",
  "risk": "low",
  "disclosure": "deferred",
  "keywords": ["definition", "references", "diagnostics", "impact"]
}
```

Do not include the full input schema for every match by default.

Provide exact expansion through one of these acceptable patterns:

- `tool_describe({name})`;
- `tool_search({query, detail:"schema", name:"..."})`;
- a similarly bounded exact-name contract.

Requirements:

- exact expansion only for an already policy-allowed/discoverable tool;
- hidden/denied tools cannot be described as a bypass;
- expansion returns current canonical/wire-safe input schema;
- output is bounded and indicates truncation only if the schema itself violates configured tool-schema bounds;
- no credentials/backend endpoints/internal secret config are returned.

### LSP semantic facades

Investigate a grouped surface along these lines; exact names may vary with repo conventions:

- `lsp_query`: definition/reference/hover/symbols/diagnostics/capabilities;
- `lsp_context`: semantic context, hunk context, call/type hierarchy, impact-oriented reads;
- `lsp_preview`: rename/format/source/code-action/semantic-check preview creation;
- `lsp_workflow`: repair/review/security/interface/cross-file recipes, deferred.

Every facade must call the same `LspService`/existing helper paths and return compatible structured data/provenance. Avoid copying operation implementations.

If repository evidence shows one or two facades are enough, prefer fewer. The goal is conditional schema relevance, not arbitrary taxonomy.

### Git semantic facades

Investigate:

- a model-visible read-only `git_read` or equivalent for status/diff/log/show/blame/refs/worktree state;
- `git` retained for typed mutations/recovery/network/destructive operations.

Do not expose the existing hidden program-only adapter directly if its contract was intentionally narrower or caller-restricted. Reuse its service/helpers where possible but define a model-facing contract deliberately.

Raw `subcommand` compatibility may remain on `git`; new prompts/descriptions should prefer typed semantic actions.

### Task semantic facades

Investigate separating common delegation/control from advanced orchestration:

- common `task`: spawn/status/message/wait/cancel/interrupt;
- deferred `task_group` for spawn_many/create_group/status_group/wait_group/cancel_group;
- deferred `converge` or equivalent bounded convergence family.

Alternatively retain one canonical `task` but expose schema variants/adapters by profile if the provider abstraction supports this without alias ambiguity.

The underlying run IDs, owner lineage, scheduler, run control, worktree isolation, and convergence store remain unchanged.

## 7. Ordered work packages

### Work package A — Schema/token census

Measure serialized description+parameter sizes for every initially advertised/deferred tool and specifically `git`, `lsp`, `task`, `work_order`, and search results.

Record operation counts and parameter relevance by operation family.

Acceptance evidence: closure contains before/after byte/token estimates using a deterministic approximation; no permanent benchmark gate.

### Work package B — Compact discovery result

Remove full schemas from broad search and add exact schema expansion.

Acceptance evidence:

- broad query returning ten large tools remains bounded substantially below baseline;
- model can search -> choose -> describe -> call;
- denied/hidden negative tests pass.

### Work package C — LSP facade experiment and implementation

Use deterministic fixtures to compare selection accuracy/schema size for current `lsp` vs candidate grouped facades. Implement only groupings that materially improve clarity without duplicating backend logic.

Acceptance evidence: each facade delegates canonical LSP operations and no copy of operation execution logic appears.

### Work package D — Git facade experiment and implementation

Add or expose a deliberate read-only Git model surface if the census supports it.

Acceptance evidence: read-only parent ceiling can admit Git reads without granting Git writes; all mutation/recovery paths still use canonical Git mutation owner.

### Work package E — Task advanced-surface deferral

Move group/convergence schema weight out of ordinary delegation if this can be done compatibly.

Acceptance evidence: ordinary spawn/wait flow sees a smaller schema; advanced orchestration remains discoverable and uses the same durable services.

### Work package F — Compatibility/profile/docs

Update disclosure/profile tables, provider alias tests, and architecture docs. Existing canonical multiplexed names may remain as compatibility/deferred surfaces until a later evidence-based removal plan.

## 8. Failure, cancellation, restart, and contention semantics

Discovery/description are read-only and bounded.

New Git/LSP/task facades inherit failure/cancellation/restart semantics from canonical implementations. They must not catch typed errors and retry through a different backend.

Task group/convergence cancellation and restart behavior remain durable run-control/store behavior. Git mutation recovery remains operation-state/recover behavior. LSP server restart/backoff remains LSP-service behavior.

## 9. Compatibility and migration

No storage migration.

Tool names should be additive. Do not remove `git`, `lsp`, or `task` in M003. If new facades eventually make old model-facing schemas redundant, closure may recommend a later deprecation plan with transcript/config/agent-profile census.

Provider wire aliases must remain collision-free.

## 10. Required tests

### Focused unit tests

- compact `tool_search` output bounds;
- exact schema-description authorization;
- schema description reflects latest registry metadata;
- hidden/denied description rejection;
- facade input validation by operation family.

### Integration tests

- search -> describe -> invoke for deferred tool;
- LSP query/context/preview facade against fake/local LSP provider;
- Git read-only facade status/diff/log under read-only ceiling;
- Git mutation still requires write authority;
- task common spawn/wait/control;
- advanced group/convergence remains discoverable and functional.

### Restart/recovery tests

Reuse existing LSP/Git/task restart tests where facades simply delegate. Add no duplicate recovery state.

### Security/negative tests

- schema description cannot reveal hidden MCP/internal tools;
- Git read facade cannot smuggle mutation;
- LSP preview remains non-mutating;
- common task facade cannot invoke group/convergence fields accidentally;
- provider alias collisions fail explicitly.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tool::tool_search
cargo test -p codegg --lib tool::lsp
cargo test -p codegg --lib tool::git
cargo test -p codegg --lib tool::task
cargo test --test tool_surface_minimization
cargo test --test tool_program_git_lsp_palette
cargo test --test agent_run_tool
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Use current target names at implementation time.

## 12. Documentation updates

- `architecture/tool.md`
- `architecture/agent-tool-surface.md`
- `architecture/lsp.md`
- Git architecture docs
- agent-run/delegation/convergence docs
- model adapter docs if new canonical/wire aliases are added

## 13. Acceptance criteria

M003 closes when broad tool discovery is compact, full schemas are loaded only for selected tools, and the largest multiplexed coding surfaces have been reduced into evidence-backed semantic groupings without creating duplicate execution/recovery authorities or breaking compatibility.

## 14. Stop conditions

Stop if:

- M001/M002 are not closed;
- a facade requires copying backend/execution logic;
- provider limitations require a protocol-level dynamic schema mechanism;
- schema splitting creates canonical-name ambiguity or breaks persisted tool-call compatibility;
- measured schema reduction is trivial and does not justify the extra model-facing names.

## 15. Closure evidence required

Include:

- before/after schema-size census;
- discovery search/describe trajectory;
- facade delegation map to canonical owners;
- compatibility-name disposition;
- representative ToolFragile/LocalStrict and Frontier profile definition sets;
- security/authority negatives;
- exact verification results;
- residual findings.

## 16. Handoff notes

The target is lower decision entropy, not maximum tool count reduction. Prefer a few semantically coherent tools whose parameters are mostly relevant to each call over either one giant union schema or dozens of tiny one-operation wrappers.
