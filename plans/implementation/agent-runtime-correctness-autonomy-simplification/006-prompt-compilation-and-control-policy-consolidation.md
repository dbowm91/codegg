# Agent Runtime Correctness, Autonomy, and Simplification M006 — Prompt Compilation and Control-Policy Consolidation

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- Milestone M006

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Primary class: maintainability/reliability simplification

Dependencies:

- hard: M005 agent-loop recovery and autonomy state machine
- interface: `PromptCompiler`, model-profile adapter resolution, resolved tool surface, task-state policy

Relevant references:

- `plans/000-long-term-specification.md`
- `plans/003-planning-process.md`
- historical `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md`
- `architecture/agent.md`
- `architecture/cache-aware-context.md`
- `architecture/provider.md`

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/006-status.md`

## 1. Objective

Make the prompt compiler the sole authoritative startup behavior-contract composition path and remove duplicated startup control policy that currently mutates provider message history after compilation.

Reduce prompt entropy and drift without removing meaningful model adaptation, project instructions, runtime-asset snapshots, dynamic steering, todo reminders, or recovery controls.

## 2. Explicit non-goals

Do not:

- remove project/user instructions, skills, runtime assets, agent roles, or model-adapter behavior;
- optimize prompts solely for token count at the expense of explicit safety/authority contracts;
- hard-code one universal prompt for all models;
- move dynamic steering, runtime notifications, or stateful recovery messages into a supposedly stable startup prefix;
- expose fewer actual tools to the model merely to shorten text; tool-surface changes belong to execution policy;
- introduce a prompt DSL/template engine;
- add snapshot/golden tests for every byte of every prompt if semantic block tests are sufficient;
- reintroduce provider-specific message mutation in several call sites;
- change public config instruction semantics without compatibility evidence.

## 3. Current implementation evidence

Inspect at minimum:

- `src/agent/prompt.rs` prompt compiler, prompt blocks, block ordering/cache classes, legacy prompt loaders, harness/role/profile/capability contracts;
- `crates/codegg-core/src/model_profile/policy.rs` startup profile policy and late control injection;
- `src/agent/turn_runtime.rs` prompt compiler inputs/runtime blocks;
- `src/agent/loop.rs` startup policy application, todo reminders, recovery/steering control instructions, plan-mode changes;
- resolved tool-surface and capability APIs;
- runtime asset snapshot/pin assembly;
- prompt/cache tests and architecture docs.

Baseline duplication/drift includes:

- `PromptCompiler` emits a harness contract, global planning/todo contract, role contract, optional subagent output contract, model-profile contract, plan-mode contract, web/research capability contracts, agent instructions, identity statement, textual `Available tools`, textual `Available skills`, and `Using model` text;
- `apply_startup_profile_policy()` separately injects tool-use contract, small-patch discipline, and todo discipline into provider messages;
- plan-mode contract contains a hard-coded tool list even though the resolved tool surface is authoritative;
- textual available-tool lists duplicate provider tool schemas;
- websearch contract includes backend/environment implementation details that are often irrelevant to model behavior;
- stable startup behavior can therefore be split between compiled system text and post-compilation message mutation, complicating caching/fingerprints and making duplicate instructions possible.

## 4. Invariants that cannot regress

- one deterministic compiler/build path owns startup prompt/control contracts;
- model-profile differences remain explicit and testable;
- resolved tool surface, not hard-coded prompt text, is authoritative for actual available tools;
- plan mode cannot advertise mutating/unavailable tools;
- runtime-asset snapshot identity and project instructions remain immutable for an in-flight turn;
- stable/slow-changing/volatile cache classes remain semantically accurate;
- dynamic steering, recovery, permission, todo reminder, notification, and compaction control messages remain late/volatile when they genuinely depend on runtime state;
- prompt compilation remains pure with respect to network and process-global CWD;
- providers that prefer user control messages or disallow late system messages still receive compatible dynamic controls;
- deleting redundant text must not delete a safety rule that exists nowhere else.

## 5. Target prompt ownership

The startup prompt should be assembled conceptually as:

```text
required harness invariants
+ agent role/output contract
+ model/profile deltas that materially change behavior
+ mode-specific contract derived from resolved capabilities
+ project/runtime-asset/user instructions
+ optional stable capability guidance that is not already encoded by tool schemas
```

Dynamic controls remain separate:

```text
user steering
recovery/stall correction
current todo reminder
background notification
compaction context frame
permission/user-interaction dependent control
```

`PromptCompiler` should own startup profile deltas currently added by `apply_startup_profile_policy()`. `push_control_instruction()` or equivalent may remain for genuinely dynamic controls and provider-specific placement rules.

## 6. Redundancy review requirements

Review each current prompt block against an authoritative source.

Default disposition to evaluate:

- base harness contract: retain, tighten to durable invariants;
- goal/todo global contract: inject only for agents/profiles with those surfaces and keep concise;
- role contract: retain;
- subagent output contract: retain where subagent role requires it;
- model-profile contract: retain only behavioral deltas, not descriptive labels;
- plan-mode contract: retain but derive advertised capabilities from resolved surface; remove hard-coded static tool inventory;
- websearch contract: keep behavioral rule (`use websearch/webfetch appropriately`) but remove provider-key/backend enumeration unless the model needs it;
- research-subagent contract: retain only when `task` + research subagent are actually available;
- agent custom system prompt: retain;
- duplicate identity line: merge with role contract if no distinct value;
- `Available tools: ...`: delete if provider schemas and mode contract are sufficient; keep only if a specific provider/model demonstrably benefits and profile-gate it;
- `Available skills: ...`: keep only if skill names are not otherwise discoverable and materially useful;
- `Using model: ...`: delete unless a concrete model-aware task contract needs it;
- startup tool-use/small-patch/todo policy: convert to compiler profile blocks or delete when already expressed by harness/role.

## 7. Compiler and cache requirements

- keep block identities stable enough for cache diagnostics but do not preserve obsolete block IDs purely for history;
- prompt fingerprint must cover all startup behavioral content after consolidation;
- no startup behavior may be injected later without being reflected in the intended cache identity unless it is deliberately volatile;
- duplicate block identity diagnostics should remain useful;
- compiler should consume resolved tool/capability data once rather than receive one list for prompt text and a different list for provider schemas;
- sort/determinism behavior remains intact.

## 8. Legacy prompt-loader disposition

Audit:

- `load_agent_prompt`;
- `load_agent_prompt_with_context`;
- `load_agent_prompt_with_snapshot`;
- `assemble_system_prompt_with_profile`;
- all production/standalone/test callers.

Preferred outcome:

- production uses `PromptCompiler` only;
- compatibility helpers used only by isolated standalone/test paths are either migrated or clearly marked non-production;
- deprecated CWD-oriented prompt loading is deleted once no real caller remains;
- do not retain several full prompt assembly functions that can diverge.

If one public library API still exposes legacy prompt assembly, preserve a thin adapter that delegates into the compiler rather than maintaining separate assembly logic.

## 9. Ordered work packages

### Work package A — Prompt source inventory

1. enumerate every startup and dynamic control injection point;
2. map each text contract to owner, cache class, trigger, and duplicate source;
3. classify startup versus dynamic;
4. record which profiles/providers require user-message placement instead of late system placement;
5. capture representative compiled prompts for build/plan/research/security/tool-fragile profiles for semantic comparison.

### Work package B — Move startup profile policy into compiler

1. represent tool-use/small-patch/todo startup deltas as profile-derived prompt blocks or equivalent compiler inputs;
2. remove `apply_startup_profile_policy()` from the production post-compile message mutation path;
3. preserve dynamic `push_control_instruction()` placement rules;
4. ensure compiler fingerprint now includes the moved content;
5. delete duplicate startup policy helpers/tests that no longer have a caller.

### Work package C — Remove redundant capability text

1. derive plan-mode capability text from resolved surface rather than hard-coded names;
2. delete/reduce textual tool list where provider schemas are authoritative;
3. remove model-name display text unless proven useful;
4. simplify websearch backend detail to behaviorally useful guidance;
5. gate goal/todo/research guidance on actual capability availability;
6. merge identity/role text where redundant.

### Work package D — Retire parallel prompt assemblers

1. migrate remaining production/standalone callers to compiler;
2. delete deprecated CWD prompt loader when unused;
3. preserve thin compatibility delegation only where documented public API requires it;
4. update tests to assert semantic block presence/absence rather than several independent assembler outputs.

### Work package E — Prompt semantic tests

Add focused tests proving:

- build profile gets one tool-use/patch contract, not duplicates;
- plan mode advertises only resolved read-only/planning capabilities;
- no textual available-tool list diverges from provider surface;
- research guidance appears only when spawnable;
- no goal/todo guidance for agents without those capabilities, if applicable;
- tool-fragile profile retains necessary structured-call guidance;
- dynamic control placement still respects providers that avoid late system messages;
- compiler fingerprint changes when a startup profile policy changes;
- dynamic recovery message does not mutate stable prompt fingerprint.

### Work package F — Documentation

Update:

- `architecture/agent.md`;
- `architecture/cache-aware-context.md`;
- `architecture/provider.md` if message-placement/adaptation ownership changes.

Document ownership/invariants, not exact prompt prose.

## 10. Storage, protocol, migration, and compatibility effects

Storage/protocol:

- none expected.

Config compatibility:

- existing `config.instructions`, agent prompts, skills, and project instructions remain supported;
- model profiles retain behavioral semantics even if internal prompt block placement changes;
- no user migration should be required.

Caching:

- prompt fingerprints/cache boundaries may change once because startup blocks are consolidated. This is expected and should be recorded; persistent semantic state must not depend on the old fingerprint value.

## 11. Concurrency, cancellation, and failure semantics

- compilation remains pure and per-turn;
- immutable asset snapshots remain captured before compilation;
- dynamic control insertion remains turn-local;
- no network fetch or asset refresh occurs inside compiler;
- if runtime capability resolution fails, fail/omit according to existing tool-surface semantics rather than advertising a guessed static list.

## 12. Focused verification

Run focused prompt/model-profile tests plus representative snapshots/semantic assertions.

Suggested checks:

```text
compiler deterministic ordering/fingerprint
startup policy appears exactly once
plan-mode surface derived from actual capabilities
research guidance gating
legacy loader production call-site search
late control placement for user-control-message profiles
```

Then run:

```bash
scripts/verify.sh quick
```

Do not add a full prompt corpus snapshot suite or token-budget CI gate.

## 13. Static guards

Do not add a script scanning prompt strings for duplicates.

Type/block ownership and semantic tests are the desired enforcement. Existing runtime-asset/CWD guards should only be updated if deleted legacy functions change their allowlists.

## 14. Acceptance criteria

M006 closes only when:

- `PromptCompiler` is the sole production startup behavior-contract composition path;
- startup profile policy is no longer separately mutating provider messages after compilation;
- plan-mode advertised capabilities derive from the resolved surface;
- redundant textual tool/model/backend details are removed unless a profile-specific need is proven;
- goal/todo/research guidance is capability-gated;
- legacy parallel prompt assemblers are deleted or reduced to thin compiler adapters with justified callers;
- compiler fingerprint covers consolidated startup behavior;
- dynamic control messages retain provider-compatible placement and remain outside stable startup identity when appropriate;
- focused semantic tests and `scripts/verify.sh quick` pass;
- prompt source/branch count decreases rather than being replaced by a new template framework.

## 15. Stop conditions

Stop and split a follow-up if:

- a provider API fundamentally cannot represent the consolidated startup system content without a provider-specific adapter change;
- a public library API requires a versioned legacy prompt-assembler compatibility surface;
- capability-derived plan-mode text requires a broader tool-surface redesign rather than using the existing resolved surface.

Do not reintroduce a second startup mutation pass as the fallback.

## 16. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/006-status.md` must include:

- implementation commit/PR;
- before/after startup prompt/control source inventory;
- deleted/merged prompt block table;
- profile-specific retained guidance rationale;
- legacy prompt-loader disposition;
- representative semantic prompt tests and fingerprint evidence;
- quick verification result;
- any prompt-cache compatibility note and unresolved findings by severity.
