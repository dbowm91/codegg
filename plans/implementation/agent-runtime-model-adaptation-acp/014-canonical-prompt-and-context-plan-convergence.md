# Agent Runtime, Model Adaptation, and ACP Milestone 014 — Canonical Prompt and Context-Plan Convergence

Status: implemented — closure record: `plans/closure/agent-runtime-model-adaptation-acp/014-status.md`

Repository baseline: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`

Source corrective addendum:

- `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-014--canonical-prompt-and-context-plan-convergence`

Historical plans corrected by this milestone:

- `plans/implementation/agent-runtime-model-adaptation-acp/001-prompt-compilation-and-agent-registry-correctness.md`
- `plans/implementation/agent-runtime-model-adaptation-acp/009-context-plan-and-cache-convergence.md`

Primary class: invariant/correctness

## 1. Objective

Make one typed prompt/context construction path authoritative for root and descendant turns. Every behavior-affecting system block—harness, role, instructions, skills, memory, goal, security evidence, research plan/evidence, LSP context, Git context, planning guidance, and model-adapter fragments—must be assembled before prompt compilation and context/cache identity are finalized.

The milestone must remove manual post-compilation system-string mutation and the duplicate plan-mode contract while preserving provider message chronology, tool-call/result pairing, immutable asset pinning, and conservative context behavior.

## 2. Dependencies

Hard dependency:

- Milestone 013 strict closure, because security/research preparation, coordination, and finalization must expose stable typed context inputs before this milestone assigns prompt/cache identity.

Existing foundations:

- `PromptCompiler` and `PromptCompilerInput` are the declared production prompt entry point;
- `CompiledPrompt` carries compiler version, blocks, flattened text, and fingerprint;
- `ContextPlan` preserves chronological provider messages, tool definitions, cache classes, fingerprints, and cache identity;
- root turn execution already resolves assets, tools, model adapter, memory, goals, LSP, Git, and specialized runtime inputs;
- descendant execution already calls `PromptCompiler` but does not consistently carry explicit execution/snapshot/pin/runtime context;
- context packing/policy remains conservative and can restore the base tool surface.

## 3. Current implementation evidence

Re-audit at implementation time. At the reviewed baseline:

- root execution calls `PromptCompiler::compile` with `runtime_context: &[]`;
- after compilation, root execution manually appends learned memory, security evidence, research plan, goal context, LSP context, Git context, and another plan-mode contract to the flattened system string;
- `assemble_system_prompt_with_profile` already includes plan-mode guidance when `is_plan_mode` is true, so root planning guidance is duplicated;
- those appended blocks are not represented as individual `PromptBlock` values and are not included in the compiler fingerprint;
- context/cache compiler identity is often derived from a hash of the flattened system message rather than the actual compiler fingerprint and block metadata;
- root and descendant turns use the same compiler function but not the same complete typed inputs;
- descendant compilation may pass no explicit execution context, asset snapshot, or pin even when those identities are available through the parent request/pool;
- legacy prompt helpers remain available and some contain remote-instruction placeholders, though production is intended to use the compiler.

## 4. Invariants that must not regress

- `PromptCompiler` is the sole production entry point for effective system context.
- Root and descendant turns consume the same block types, ordering rules, and fingerprint algorithm.
- Every provider-behavior-affecting system block changes prompt/compiler identity.
- Active turns remain pinned to immutable asset snapshots and adapter/tool-surface identities.
- Instructions and skills are resolved from explicit project/runtime assets, not process-global cwd.
- Remote instructions are fetched/published by the runtime-asset owner; the compiler performs no network I/O and never presents a URL placeholder as loaded content.
- Planning guidance appears exactly once.
- Tool names in the prompt match the canonical resolved tool surface.
- Provider message chronology and assistant-tool-result adjacency remain unchanged.
- Private reasoning is not placed into visible prompt blocks or diagnostics.
- Context policy cannot omit required protocol messages, active specialized evidence required for correctness, or the current user request.
- Large evidence remains bounded or handle-backed.

## 5. Scope

### In scope

- Replace untyped `runtime_context: &[String]` with typed prompt/context block inputs or an equivalent bounded structure.
- Define block kinds, stable ordering, cache class, required/optional status, source identity, and bounded content rules.
- Build memory, goal, security, research, LSP, Git, instructions, skills, and planning blocks before compilation.
- Include resolved adapter/tool-surface/asset/runtime identities in compiler and context-plan fingerprints.
- Remove manual post-compilation `system.push_str` calls from production root execution.
- Remove duplicate plan-mode guidance.
- Pass explicit execution/snapshot/pin/runtime block identity to descendant compilation where available.
- Make `ContextPlan` consume the real compiler fingerprint and typed block ordering.
- Retire or hard-deprecate legacy helpers that can bypass canonical compilation.
- Add root/child golden and cache-identity fixtures.

### Explicitly out of scope

- New memory, goal, LSP, Git, security, or research features.
- Aggressive lossy context packing or automatic semantic summarization.
- Provider-specific message-order changes.
- Storage migration or a new context database.
- Replacing the existing asset snapshot/pin subsystem.
- Broad prompt rewriting unrelated to convergence.
- Exposing private reasoning or full evidence bodies.

## 6. Required production changes

### Typed prompt blocks

Extend or replace `PromptBlock` with a bounded type such as:

```rust
pub struct PromptBlock {
    pub kind: PromptBlockKind,
    pub source_id: String,
    pub cache_class: CacheClass,
    pub required: bool,
    pub content: String,
    pub content_hash: String,
}
```

Suggested kinds:

- harness contract;
- role/output contract;
- model-adapter fragment;
- tool/agent/skill capability contract;
- project/global instructions;
- memory summary;
- active goal/checkpoint;
- security evidence summary;
- research plan/evidence summary;
- LSP context;
- Git context;
- plan-mode contract;
- bounded control/recovery instruction.

Do not make the enum a general plugin language. Unknown plugin/custom context should enter through one bounded extension kind with explicit source identity and visibility.

### Ordering

Define deterministic ordering before flattening:

1. stable harness and role contracts;
2. model-adapter and effective capability contracts;
3. immutable project/global instructions and skills;
4. slow-changing memory/goal/project metadata;
5. specialized evidence and task-specific LSP/Git context;
6. plan/control guidance that must remain near the active turn where model semantics require it.

Preserve provider requirements for late system/control messages through existing adapter metadata. If all blocks must currently flatten to one system string, retain block boundaries/fingerprints internally and flatten in this deterministic order.

### Root turn assembly

Resolve all context before `PromptCompiler::compile`:

- memory store summary;
- active goal/checkpoint;
- prepared security bundle summary;
- research plan/evidence summary from M013;
- LSP context;
- Git context;
- asset instructions and skills;
- plan-mode state.

Then compile once and pass the result to the provider request without later string mutation. Any asynchronous collectors run before compilation and are cancellation-aware.

### Descendant assembly

- carry the explicit workspace/execution context from parent request/pool;
- carry available immutable asset snapshot/pin or a bounded child runtime snapshot derived before enqueue;
- represent inherited lineage/delegation context only when it affects model behavior;
- use the same block ordering and fingerprint rules;
- do not fall back to cwd-based instructions or a thinner prompt path.

### Fingerprints and cache identity

The prompt fingerprint must include:

- compiler version;
- ordered block kind/source/content hashes;
- immutable asset snapshot fingerprint/pin identity;
- resolved adapter ID/version/fingerprint and reasoning mode;
- resolved tool-surface fingerprint;
- selected agent source/extends digest;
- plan-mode/specialized-runtime mode;
- any required provider control-role behavior.

`ContextPlan::CacheIdentity` must consume this compiler fingerprint directly. Do not infer it by re-hashing the flattened system message alone. The cache key remains bounded and content bodies remain absent from logs.

### Context policy

- mark required specialized evidence and current active state as non-omittable;
- optional memory/history summaries may be omitted only through existing conservative policy with diagnostics/recovery handles;
- tool definitions remain tied to resolved tool-surface identity;
- full base context/palette restoration remains available after starvation or uncertainty;
- context-plan application must preserve chronological messages and private-reasoning round-trip state without exposing it in diagnostics.

### Legacy helpers

Inventory `assemble_system_prompt`, `load_agent_prompt*`, `base_prompt_parts`, and deprecated cwd-based helpers. Production callers must route through the compiler. Retain only clearly documented test/compatibility helpers; add a focused static guard or unit-level call-site test rather than a broad brittle grep if needed.

## 7. Ordered work packages

### Work package A — Block inventory and typed contract

- inventory every root/child system-string append and control injection;
- classify kind, source, cache class, required status, bounds, and owner;
- define typed block structures and deterministic ordering;
- add fixtures for complete root and descendant block sets.

Acceptance evidence:

- no behavior-affecting system append is unclassified;
- block order/fingerprint is deterministic across runs;
- private reasoning and secret-bearing fields are excluded.

### Work package B — Root pre-compilation convergence

- collect memory/goal/security/research/LSP/Git context before compile;
- pass typed blocks to compiler;
- remove post-compile string mutation;
- remove duplicate plan contract;
- use real compiler fingerprint in context plan/cache identity.

Acceptance evidence:

- root prompt contains each expected block exactly once;
- changing any block changes prompt/cache identity;
- unchanged inputs produce stable identity.

### Work package C — Descendant convergence

- propagate explicit execution/asset identity;
- compile child prompt with the same block contract;
- verify child tool/agent descriptions match effective surface;
- remove thinner/legacy production paths.

Acceptance evidence:

- equivalent root/child agent inputs produce equivalent shared contracts;
- child cannot resolve instructions from process cwd;
- asset refresh after enqueue cannot alter active child prompt identity.

### Work package D — Context policy and compatibility

- mark required blocks and conservative omission rules;
- ensure full-context restoration works;
- preserve tool protocol chronology and reasoning round trip;
- update cache diagnostics to use fingerprints/metadata only.

Acceptance evidence:

- required block cannot be omitted;
- cache-key separation covers adapter/tool/prompt/mode changes;
- diagnostics contain no private content.

### Work package E — Documentation and closure handoff

- update prompt, cache/context, agent, and runtime architecture;
- create M014 closure record only after independent review;
- promote M015 only on strict closure.

## 8. Failure, cancellation, restart, and contention semantics

- Context collector failure is typed as required failure or optional omission/evidence gap according to block policy.
- Cancellation during asynchronous memory/goal/LSP/Git/security/research collection prevents provider invocation and does not publish a partial prompt as final.
- Compilation is pure and deterministic after inputs are collected.
- Asset snapshots/pins are captured before asynchronous turn execution and remain immutable for the turn.
- Concurrent turns may share immutable blocks/snapshots but cannot mutate another turn's block list or identity.
- Context-policy failure/uncertainty restores the full required request rather than silently dropping blocks.
- Daemon restart behavior remains governed by existing transient-turn policy; no new persistence is introduced.

## 9. Compatibility and migration

- Existing agent/config/instruction/skill formats remain readable.
- Provider request DTOs may continue receiving one flattened system string if required; typed blocks remain internal metadata.
- Native protocol remains unchanged unless bounded prompt/cache diagnostics are already exposed additively.
- No durable storage migration is required.
- Existing context-packer configuration remains conservative and compatible.
- Legacy prompt helpers may remain deprecated for tests but must not be production authority paths.

## 10. Required tests

### Prompt block tests

- deterministic order and fingerprint;
- every block kind bound/source identity;
- plan-mode contract appears once;
- custom agent system prompt does not suppress role/output contracts;
- remote instruction URL is absent unless asset owner supplied fetched content;
- private reasoning and secrets absent.

### Root production-shaped tests

- ordinary root with instructions/skills/memory/goal/LSP/Git;
- security root with prepared evidence;
- research root with coordinated evidence summary;
- plan-mode root;
- asset snapshot refresh during turn leaves prompt unchanged.

### Descendant tests

- child receives canonical harness/role/tool/agent contracts;
- child uses explicit workspace and asset identity;
- parent/child tool-surface changes alter fingerprint correctly;
- no cwd-backed instruction resolution.

### Cache/context tests

- adapter, tool surface, compiler, asset, specialized mode, and reasoning mode separate cache keys;
- unchanged inputs retain key;
- required blocks survive packing/policy;
- chronological assistant/tool history remains intact;
- full restoration after starvation/uncertainty;
- diagnostics contain hashes/counts only.

### Negative tests

- missing required security/research evidence prevents compile/provider call;
- oversized optional block truncates or becomes handle;
- duplicate source IDs/kinds are diagnosed deterministically;
- legacy helper cannot be reached from production root/child call sites.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets
cargo test -p codegg agent::prompt
cargo test -p codegg context::plan
cargo test --test context_plan_convergence -- --test-threads=4
cargo test --test agent_loop_harness -- --test-threads=4
cargo test --test subagent -- --test-threads=4
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_project_agent_pwd_inference.py
python3 scripts/check_builtin_agents.py
python3 scripts/generate_builtin_agents.py --check
```

Add focused golden tests rather than a new large snapshot framework. Run the canonical quick verification command at handoff; do not add a broad CI matrix.

## 12. Documentation updates

- `architecture/agent.md`: complete prompt compiler inputs and root/child equivalence;
- `architecture/cache-aware-context.md`: block classes, fingerprints, required/optional semantics, and cache identity;
- prompt/runtime documentation: no post-compilation mutation and one plan contract;
- specialized runtime docs: how validated evidence enters typed blocks;
- corrective addendum, registry, and M014 closure record.

## 13. Acceptance criteria

- All effective root/child system context is represented before compilation.
- No production `system.push_str`-style post-compilation mutation remains.
- Plan-mode guidance appears once.
- Root and descendants use the same typed block/order/fingerprint contract.
- Compiler/cache identity includes asset, adapter, tool surface, agent, mode, and all behavior-affecting blocks.
- Context plan preserves message/tool chronology and private reasoning round trips.
- Required blocks cannot be silently omitted.
- Legacy/cwd prompt paths are not production authorities.
- Focused prompt/context/root/child fixtures pass.

## 14. Stop conditions

Stop and report if:

- provider support requires a breaking message DTO redesign rather than internal typed blocks plus flattening;
- context convergence requires new memory/goal/LSP/Git features;
- correct block identity requires serializing private reasoning or secret content;
- a change belongs to runtime-assets/session-projections rather than this compiler boundary;
- aggressive semantic compaction is necessary for closure;
- durable prompt storage or AgentRun persistence becomes necessary.

## 15. Required closure evidence

The closure record must include:

- complete pre/post block inventory;
- root and descendant block/fingerprint fixtures;
- duplicate plan-contract removal evidence;
- cache-key separation matrix;
- required-block/chronology/private-content evidence;
- legacy production call-site audit;
- focused command results and exact commits;
- remaining low-severity limitations;
- explicit recommendation to promote or block Milestone 015.
