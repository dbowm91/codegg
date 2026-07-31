# Agent Runtime, Model Adaptation, and ACP Milestone 001 — Prompt Compilation and Agent Registry Correctness

Status: ready

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-001--prompt-compilation-and-agent-registry-correctness`

Long-term requirements:

- `plans/000-long-term-specification.md#4.4-frontends-render-projections`
- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- No new ADR is required if the milestone only converges existing prompt/agent paths. Stop and propose an ADR if implementation would redefine daemon authority, public provider message semantics, or the final durable agent-run model.

Primary class: invariant/correctness

## 1. Objective

Create one canonical prompt-compilation path used by production root turns and descendant turns, and correct the agent registry so compiled built-ins, global/project files, config overlays, inheritance, runtime kinds, permissions, fallback models, and immutable runtime-asset snapshots resolve consistently.

The milestone must remove the current discrepancy in which the strongest profile-aware prompt composer is tested but bypassed by production turn construction, while descendant turns use an even thinner prompt path. It must also make documented agent merge/replace behavior match runtime behavior and establish explicit `extends` semantics for safely customizing built-in agents such as `security-review`.

## 2. Dependency readiness

Hard dependencies are closed:

- runtime-asset snapshots and immutable turn pinning are closed;
- session projection and daemon turn execution foundations are closed;
- the current agent registry, generated built-ins, prompt modules, root turn runtime, and descendant worker path exist.

No external service or protocol dependency blocks this milestone.

This milestone is the first dependency-ready handoff. Later milestones must not begin by adding new prompt or agent-resolution paths around these defects.

## 3. Current implementation evidence

The implementation agent must verify the baseline rather than assuming this list remains exact:

- `assets/agents/*.toml` and `assets/prompts/agents/*.md` are compiled by `scripts/generate_builtin_agents.py` into `src/agent/builtins/generated.rs`.
- `src/agent/mod.rs` defines `Agent`, `AgentMode`, `AgentRuntimeKind`, overlay helpers, config conversion, and file-based resolution.
- `resolve_agents_with_context` currently replaces an existing entry with a loaded file agent rather than applying the documented overlay behavior.
- `Agent::merge_overlay` does not consistently carry every relevant field, including fields such as fallback model and runtime specialization.
- some runtime-kind parsing paths convert invalid values to `None` rather than producing a source-aware error.
- `src/agent/prompt.rs` contains `assemble_system_prompt_with_profile`, role contracts, planning guidance, web/research guidance, and model-profile guidance.
- `src/agent/turn_runtime.rs` assembles production root prompts through `load_agent_prompt_with_snapshot` or `load_agent_prompt_with_context`, then appends memory, goal, LSP, Git, and plan text manually.
- `src/agent/worker.rs` constructs descendant messages directly from the selected agent prompt plus delegated user text.
- context-aware prompt loaders represent remote instruction URLs as placeholders while the asynchronous fetch path exists elsewhere.
- `ResolvedAgentExecutionProfile::resolve` documents fallback behavior that does not match all ordinary selection paths.

## 4. Invariants that must not regress

- Root and descendant turns use one compiler contract and differ only by resolved inputs/policy.
- Prompt assembly receives an explicit `ExecutionContext` and immutable `ProjectAssetSnapshot`/pin; it does not infer project identity from process-global cwd.
- Prompt content cannot grant tools or authority not present in the resolved runtime surface.
- Built-in definitions remain generated from declarative assets.
- Global/project custom agents cannot widen the hard safety ceiling.
- Invalid higher-precedence definitions do not silently erase a valid lower-precedence agent without a typed fail-closed policy and diagnostic.
- Active turns remain pinned to the captured asset generation and do not observe later refreshes.
- Provider-private hidden reasoning is not introduced into user-visible prompt content.
- Existing native agent files remain readable unless they are invalid under an explicitly documented schema rule.

## 5. Scope

### In scope

- Define one `PromptCompiler` or equivalent canonical function/object used by root and descendant turn factories.
- Compile prompt blocks from:
  - harness contract;
  - role/output contract;
  - agent prompt;
  - resolved model-profile contract;
  - available agent/tool/skill metadata supplied by later-compatible interfaces;
  - pinned project instructions/skills;
  - mode/control guidance;
  - bounded runtime context supplied by the caller.
- Preserve a structured prompt result or block list suitable for Milestone 009 rather than immediately flattening all context internally.
- Route production root and child turns through the compiler.
- Remove or deprecate duplicate legacy prompt loaders after call-site migration.
- Implement deterministic file-agent overlay semantics:
  - merge by default;
  - explicit full replacement only when supported and requested;
  - strict field-level explicitness;
  - source provenance and diagnostics.
- Add explicit agent inheritance (`extends`) for native TOML agents, including cycle detection and deterministic precedence.
- Merge all agent fields correctly, including role, mode, model, fallback model, runtime kind, prompt behavior, limits, visibility, and permissions.
- Make invalid runtime kinds, modes, overlay flags, prompt references, or inheritance chains typed configuration errors.
- Correct model selection/fallback semantics so fallback is retained as a fallback candidate rather than silently ignored or selected as ordinary precedence.
- Resolve remote instructions through one explicit bounded service or reject/diagnose unsupported runtime fetching; do not leave placeholder text pretending content was loaded.
- Add prompt and registry fingerprints/provenance for diagnostics and later cache identity.

### Explicitly out of scope

- Model-specific tool aliases or TOML model adapters.
- New nested-agent execution behavior.
- Specialized security/research host workflows.
- Active context packing or broad context omission.
- ACP implementation.
- Final durable `AgentRun` storage.
- Broad rewrite of the provider API.

## 6. Required production changes

### Agent schema and registry

Prefer extending the existing declarative registry types rather than creating a second loader. Add typed explicitness for optional fields so an overlay can distinguish absent values from deliberate false/empty values.

A native TOML agent should support a bounded form equivalent to:

```toml
schema_version = 1

[agent]
name = "rust-security-review"
extends = "security-review"
description = "Security review specialized for unsafe Rust and FFI"
mode = "subagent"

[agent.prompt]
append = "Prioritize unsafe blocks and FFI boundaries."
```

The exact syntax may use the existing tables if that avoids unnecessary migration, but semantics must be explicit and documented.

Resolution order must remain deterministic and source-aware:

1. compiled built-in;
2. inherited base chain;
3. global file overlay;
4. project file overlay;
5. config/session overlay;
6. hard runtime safety ceiling.

Reject inheritance cycles, missing bases when fail-closed policy applies, duplicate same-precedence names, unknown keys, and path escapes.

### Prompt compiler

The compiler should accept typed inputs rather than read global state. At minimum it needs:

- resolved agent;
- model/profile identity;
- immutable asset snapshot and pin;
- mode state;
- available tool/agent/skill names supplied by the caller;
- bounded custom/runtime context;
- compiler version.

It should produce deterministic ordered blocks plus a fingerprint. Milestone 001 may flatten the result when constructing `ChatRequest`, but must retain enough block identity to avoid another redesign in Milestone 009.

Do not append the same agent prompt/role contract through multiple paths. Do not tell a descendant it can spawn or call a tool unless the caller supplied that capability.

### Root and descendant integration

- Replace root turn prompt construction in `src/agent/turn_runtime.rs` with the canonical compiler.
- Replace direct descendant message construction in `src/agent/worker.rs` with the same compiler.
- Ensure descendants receive the same pinned project asset generation and explicit execution context as the parent request where policy requires inheritance.
- Preserve agent-specific temperature/top-p/thinking/reasoning fields without hard-coding model adaptation yet.

### Remote instructions

Choose one bounded behavior:

- resolve remote instructions during asset refresh into a pinned asset with timeout, size, scheme, and failure diagnostics; or
- explicitly reject remote instruction URLs in daemon-owned snapshot compilation until a dedicated fetch owner exists.

Do not perform uncontrolled network fetches during deterministic prompt compilation. Do not place placeholder text in the effective prompt as though the instruction body were present.

### Storage and protocol

No new durable table is expected. Additive diagnostics/provenance may be carried in internal turn/run metadata or existing asset pin structures if bounded and backward compatible.

Do not expose complete system prompts through session projections. Diagnostics should contain names, source kinds, digests, versions, and bounded error summaries only.

### Documentation

Update `architecture/agent.md` and relevant runtime-asset documentation to describe one prompt path, inheritance, precedence, failure behavior, and root/child equivalence.

## 7. Ordered work packages

### Work package A — Inventory and contract definition

- enumerate every production/test call site of prompt loaders/composers;
- identify every agent-loading and merge path;
- define the canonical compiler input/output and agent resolution contract;
- record compatibility behavior for existing Markdown and TOML agents;
- add failing focused tests demonstrating current root/child prompt divergence and file-overlay replacement.

Acceptance evidence:

- one call-site inventory in the implementation report;
- tests reproduce the two principal defects before correction;
- no unresolved decision about merge, replace, or inheritance semantics remains.

### Work package B — Agent resolver correctness

- add explicit declarative spec/overlay representation;
- implement `extends` resolution and cycle detection;
- make field merging complete;
- preserve source provenance and shadow diagnostics;
- reject invalid runtime kinds and unknown schema fields;
- correct fallback model representation/resolution.

Acceptance evidence:

- customizing `security-review` retains its role, runtime kind, denied mutations, and base prompt unless explicitly overridden;
- deliberate replacement resets inherited fields only when requested;
- invalid chains fail with source path and agent name.

### Work package C — Canonical prompt compiler

- implement ordered deterministic prompt blocks;
- include harness, role, model profile, agent prompt, pinned instructions/skills, and supplied capability metadata exactly once;
- produce compiler version/fingerprint;
- define bounded handling of custom instructions and remote references.

Acceptance evidence:

- stable golden fixtures for root build, plan, security-review, research, and custom inherited agents;
- same inputs produce byte-identical output/fingerprint;
- unavailable tools/agents are not advertised.

### Work package D — Production root and descendant migration

- route root turns through the compiler;
- route descendants through the compiler;
- pass explicit execution/asset inputs;
- remove/deprecate unused prompt paths and update guards/tests;
- preserve existing provider request behavior outside prompt content.

Acceptance evidence:

- root and child fixtures share the compiler and differ only in policy/input;
- no production call site reads cwd for prompt/agent resolution;
- existing turn and subagent integration tests remain green.

### Work package E — Documentation and reconciliation

- update architecture docs and example agent files;
- add source/provenance diagnostics where users can inspect effective agents;
- reconcile comments/tests that describe obsolete prompt behavior;
- record deferred dependencies for Milestones 002 and 007.

## 8. Failure, cancellation, restart, and contention semantics

- Prompt compilation is pure/bounded after asset capture; cancellation before publication returns no partial prompt.
- Failed asset/remote-instruction resolution retains the previous valid asset snapshot according to existing runtime-asset policy.
- Concurrent turns may compile independently from immutable snapshots without a global mutable prompt cache.
- Agent inheritance resolution is deterministic across restart and does not depend on directory iteration order.
- Duplicate definitions at the same precedence fail or produce one deterministic documented result; they must not race.
- Fallback model selection occurs only after a typed primary failure and does not mutate the persistent agent definition.

## 9. Compatibility and migration

- Existing built-in TOMLs remain source-compatible.
- Existing Markdown agent files remain prompt-first merge overlays; unsupported TOML-only features continue to produce explicit diagnostics.
- Existing config-defined agents retain equivalent behavior except where silent invalid values now fail clearly.
- If `replace` semantics already exist in parsed TOML, preserve their documented meaning.
- Deprecated prompt APIs may remain for one compatibility window only if production call sites are removed and deprecation tests/guards identify new use.

## 10. Required tests

Focused tests:

- field-complete overlay merge;
- explicit replacement;
- `extends` success, missing base, duplicate, and cycle;
- runtime-kind validation;
- fallback model candidate behavior;
- root/descendant prompt equivalence;
- prompt ordering and fingerprint determinism;
- unavailable tool/agent omission;
- pinned asset generation stability;
- remote instruction failure behavior;
- no cwd-dependent production prompt path.

Production-shaped tests:

- root build turn with pinned instructions and skills;
- child security-review prompt inherited from built-in plus project customization;
- project refresh after one turn affects only the next turn;
- invalid project overlay preserves or rejects according to the documented last-valid policy.

Negative/security tests:

- prompt fragment cannot expand permissions;
- inheritance cannot escape configured asset roots;
- prompt/provenance diagnostics do not disclose full secret-bearing config or complete system prompts;
- oversized prompt/remote content obeys configured bounds.

## 11. Verification commands

Focused commands should be adapted to the final test names:

```bash
cargo fmt --all -- --check
cargo test agent::prompt
cargo test agent::registry
cargo test --test subagent
cargo test --test asset_snapshot
cargo check --workspace
```

One broad local verification at handoff completion:

```bash
cargo test --workspace --lib
```

Do not add new release jobs or a large provider/model CI matrix for this milestone. Record any unrelated pre-existing broad failure separately rather than expanding scope.

## 12. Acceptance criteria

- One canonical compiler is used by production root and descendant turns.
- The profile-aware harness/role/model policy is no longer tested dead code.
- Agent merge/replace behavior matches documentation.
- Custom agents can extend built-ins safely and deterministically.
- All relevant fields merge correctly and invalid definitions fail explicitly.
- Prompt compilation uses explicit context and immutable asset snapshots.
- Remote instruction behavior is truthful and bounded.
- Prompt/compiler/agent provenance is available without disclosing full private prompt bodies.
- Existing native turn behavior remains operational.

## 13. Stop conditions

Stop and report rather than improvise if:

- implementing one prompt path requires changing the provider-neutral message model; that belongs to Milestone 008 unless a minimal compatibility change is unavoidable;
- agent inheritance requires redefining the runtime-asset precedence contract;
- the current asset snapshot does not contain enough source/spec information to resolve overlays deterministically;
- remote instruction fetching lacks an accepted network/security owner and cannot be safely moved to asset refresh;
- correcting fallback behavior requires an unplanned provider failover policy redesign;
- broad unrelated failures prevent verification after focused tests pass.

## 14. Required closure evidence

The closure record must include:

- implementation commits;
- prompt call-site inventory before/after;
- agent precedence/inheritance examples;
- root and child golden prompt evidence;
- asset pin/refresh evidence;
- focused command results and one broad local result;
- compatibility notes for existing Markdown/TOML/config agents;
- known limitations and any deferred remote-instruction behavior;
- explicit recommendation: closed, conditionally closed, corrective pass required, or blocked.
