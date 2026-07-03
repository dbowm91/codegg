# Agent Customization Correctness and Hardening Pass

## Purpose

The generated built-ins and customizable agent system now exists in the repository, including generated compiled built-ins, TOML/Markdown user agents, overlay merging, richer permissions, runtime kinds, model inheritance, and TUI inspection commands. This pass is a focused correctness review and hardening effort before treating the feature as stable.

The goal is not to add broad new functionality. The goal is to remove ambiguity, make failure modes visible, ensure safety constraints are enforced on the actual execution path, and tighten tests around the semantics that are most likely to regress.

## Current risk profile

The implementation appears to have followed the roadmap closely, but several areas deserve a corrective pass:

1. Overlay merges appear to operate on concrete `Agent` values in places where they should preserve field-level explicitness from `AgentSpec`.
2. Missing user/project `prompt_file` may degrade silently into an agent with no custom prompt.
3. Markdown agents and TOML agents may not have full parity around `replace`, `disable`, `merge`, structured permissions, and runtime/model fields.
4. `Agent::apply_safety_envelope()` exists, but the execution path must prove every selectable/spawnable agent is bounded before tool use.
5. Runtime kind and model inheritance exist, but subagent execution should be verified end-to-end against empty-model/provider-fallback regressions.
6. TUI commands exist, but `/agents diff`, `/agents validate`, `/agents reload`, and `/agent <name>` need correctness checks around stale state, hidden agents, disabled agents, and source stacks.
7. CI has agent asset checks, but local and workflow visibility should be tightened so failures are obvious.

## Non-goals

Do not redesign the entire agent system.

Do not move built-ins back to runtime TOML loading.

Do not add arbitrary executable behavior to TOML/Markdown agents.

Do not implement deep security/research preflight automation unless required to fix a correctness bug. This pass can verify runtime hooks and leave deeper behavior to a future feature pass.

## Phase 1: Preserve overlay explicitness with `AgentSpec`

### Problem

Field-level overlay semantics are fragile if custom file overlays are converted into concrete `Agent` values before merging. Concrete values cannot reliably distinguish unset fields from explicit defaults. Examples:

- `mode = "Primary"` may look like the default mode rather than an explicit override.
- `hidden = false` cannot reliably override a hidden base if the merge only checks truthiness.
- Empty or intentionally blank descriptions/prompts can be ambiguous.
- `temperature = 0.0`, `top_p = 0.0`, or `steps = 0` may be confused with absence depending on conversion choices.
- Permission maps can be merged, but source-level intent is lost once a full runtime `Agent` is created.

### Required changes

Introduce or complete a first-class merge path on declarative specs:

```rust
pub struct AgentSpec {
    pub name: Option<String>,
    pub role: Option<String>,
    pub description: Option<String>,
    pub mode: Option<AgentMode>,
    pub model: Option<String>,
    pub fallback_model: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub prompt: Option<String>,
    pub prompt_file: Option<String>,
    pub color: Option<String>,
    pub steps: Option<u32>,
    pub hidden: Option<bool>,
    pub disable: Option<bool>,
    pub replace: Option<bool>,
    pub merge: Option<bool>,
    pub runtime_kind: Option<AgentRuntimeKind>,
    pub permission: Option<AgentPermissionSpec>,
    pub source: Option<AgentSource>,
}
```

Exact names can differ, but the important property is explicit optionality. File/config layers should produce `AgentSpec`, merge specs, and only then resolve to `Agent`.

Add a deterministic merge function:

```rust
impl AgentSpec {
    pub fn merge_overlay(base: AgentSpec, overlay: AgentSpec, behavior: OverlayBehavior) -> AgentSpec;
}
```

Recommended semantics:

- `replace = true`: discard accumulated base and use overlay.
- default: merge overlay into base field-by-field.
- `Some(value)` replaces prior value.
- `None` leaves prior value unchanged.
- permissions merge per tool/pattern.
- `hidden = Some(false)` must override `hidden = Some(true)`.
- `mode = Some(Primary)` must be honored as an explicit override.
- `disable = Some(true)` removes or disables the final resolved agent.

Only after merging should the registry produce a concrete runtime `Agent`.

### Tests

Add tests proving explicit default values override base values:

- Built-in hidden `summary` overlaid with `hidden = false` becomes visible when allowed.
- Built-in `explore` overlaid with `mode = Primary` becomes selectable if explicitly configured.
- Overlay with `temperature = 0.0` keeps exactly `0.0`.
- Overlay with `description = ""` behavior is explicitly defined and tested. Prefer rejecting blank visible descriptions over silently preserving old text.
- Overlay with only `model` preserves built-in prompt, permissions, mode, role, runtime kind, and hidden state.
- Overlay with `replace = true` discards built-in prompt/permissions and emits a warning diagnostic.

### Acceptance criteria

- File/config overlays merge through `AgentSpec` or an equivalent explicit optional representation.
- Existing `Agent::merge_overlay()` is either removed, made private/test-only, or documented as a compatibility helper not used for source-layer merging.
- Tests demonstrate explicit default values are not lost.

## Phase 2: Harden diagnostics and invalid-file behavior

### Problem

Invalid user/project agent files should not panic, but they also should not silently degrade. Missing prompts, unknown fields, invalid modes, invalid runtime kinds, invalid permission actions, and bad prompt paths need source-aware diagnostics.

### Required changes

Add a consistent diagnostic path for user/project files:

```rust
pub struct AgentDiagnostic {
    pub severity: AgentDiagnosticSeverity,
    pub source: Option<AgentSource>,
    pub agent_name: Option<String>,
    pub field: Option<String>,
    pub message: String,
    pub suggestion: Option<String>,
}
```

Minimum diagnostics:

- Missing `prompt_file`: warning by default, error if the file explicitly declares `require_prompt = true` or similar later.
- `prompt_file` outside the agent file directory/project boundary: error.
- Unknown top-level TOML keys: warning for user/project, error for built-ins.
- Invalid `mode`: error, skip that layer or agent.
- Invalid `runtime_kind`: warning or error; prefer error for an explicit runtime kind.
- Invalid permission action: error, skip that permission rule or the agent depending on severity.
- Duplicate agent names in the same directory: warning or error with deterministic winner documented.
- `replace = true` against a built-in: warning.
- `disable = true` against a built-in: info/warning with source stack.

Do not silently load an agent with missing prompt_file as if it is healthy. The agent may still load if safe, but `/agents validate` and `/agents show` must surface the problem.

### Tests

Add tests for diagnostics:

- Missing prompt file emits a warning with path and field.
- Invalid runtime kind emits a diagnostic and does not crash.
- Unknown field `modell` is reported.
- Invalid permission action `sure` is reported.
- Prompt file escaping outside directory is rejected.
- Duplicate names in the same custom directory are deterministic and reported.
- `/agents validate` includes diagnostics from project files.

### Acceptance criteria

- No custom-agent parse issue is silently swallowed unless explicitly documented as ignorable.
- `/agents validate` surfaces file path, field, and actionable message.
- Built-in asset validation remains strict and generation-blocking.

## Phase 3: Verify safety envelope is enforced on hot paths

### Problem

A safety helper is insufficient if it is not applied before tool execution. Custom agents must not be able to silently escalate permissions beyond session, config, sandbox, or hard-deny policies.

### Required changes

Trace all paths that produce executable agents:

- Initial TUI/default agent list.
- `/agent <name>` selected primary/all agent.
- Agent selection dialog.
- `@mention` subagent spawn.
- `task` tool spawn.
- Auto-triggered `security-review` spawn.
- Headless/core/daemon turn runtime.
- Tests/harness helper construction.

For each path, ensure the final executable profile is produced by a single resolver that includes:

```text
resolved agent profile
+ active session model/permissions
+ config permissions
+ active sandbox/approval mode
+ hard safety policy
= executable agent profile
```

Avoid ad hoc construction of permission checkers from partial data. Prefer an explicit type:

```rust
pub struct ResolvedAgentExecutionProfile {
    pub agent: Agent,
    pub runtime_kind: AgentRuntimeKind,
    pub resolved_model: String,
    pub effective_permissions: PermissionRuleset,
    pub diagnostics: Vec<AgentDiagnostic>,
}
```

Subagent execution should receive this profile or build it through one shared function.

### Tests

Add hot-path tests:

- Project custom agent sets `edit = allow`, session denies edit; actual edit request is denied.
- Project custom agent sets `bash = allow`, session/config requires ask; actual bash permission is ask or deny according to policy.
- Hard-deny tool remains denied even when agent says allow.
- `task` spawn uses bounded permissions.
- `@mention` spawn uses bounded permissions.
- `/agent <name>` selection cannot bypass safety envelope.
- `security-review` remains mutation-denied even if project overlay tries to allow edit without explicit unsafe replacement, or replacement is diagnosed and still bounded by session policy.

### Acceptance criteria

- There is one auditable execution-profile path for primary and subagent execution.
- Tests prove custom files cannot escalate above session/config/hard policy.
- Existing permission checker still enforces final decisions; no duplicate policy engine is introduced.

## Phase 4: Model inheritance and provider fallback verification

### Problem

Model inheritance and aliases exist, but the dangerous failure mode is still an empty model string or an unintended hardcoded provider fallback during subagent execution.

### Required changes

Audit model resolution in:

- Primary agent selection.
- Subagent task execution.
- Auto security review.
- Research agent invocation.
- Tests/harness paths.

The final model resolver should return both requested and resolved model:

```rust
pub struct ResolvedModelSelection {
    pub requested: Option<String>,
    pub resolved: String,
    pub source: ModelSource,
    pub diagnostics: Vec<AgentDiagnostic>,
}
```

Model source examples:

```text
AgentExplicit
AgentRoleDefault
ParentSession
ConfigDefault
AliasResolved
FallbackModel
ProviderDefaultWithDiagnostic
```

Provider default fallback should be visible in diagnostics. Empty model strings should not be emitted in normal execution.

### Tests

- Agent explicit model wins.
- `fallback_model` is used only when primary model cannot resolve or fails according to existing error path.
- Subagent without model inherits parent/session model.
- Research all-mode agent without model can be selected and resolves through session/config.
- Provider default fallback emits a diagnostic.
- No normal `ChatRequest` is sent with an empty model for subagent execution.

### Acceptance criteria

- Subagent model resolution is deterministic and observable.
- Empty model requests are eliminated or guarded by explicit diagnostics.
- Model aliases are tested independently from provider-specific names.

## Phase 5: Markdown/TOML parity or explicit scope split

### Problem

The docs and examples describe both TOML and Markdown custom agents. If Markdown supports only prompt/frontmatter but not overlay flags, structured permissions, model fields, and runtime kinds, this must either be fixed or explicitly documented.

### Required changes

Pick one of two strategies.

Preferred strategy: parity.

Markdown frontmatter supports:

```yaml
name: my-agent
mode: subagent
description: A custom agent
model: tier.workhorse
fallback_model: tier.frontier
runtime_kind: standard
replace: false
disable: false
permission:
  read: allow
  edit: deny
bash_permission:
  action: ask
  allow_patterns:
    - cargo test*
path_permission:
  allow:
    - src/**
```

Alternative strategy: explicit split.

Markdown is documented as prompt-first and merge-only. TOML is documented as the full structured format. `/agents validate` warns when unsupported Markdown frontmatter keys are ignored.

### Tests

If parity:

- Markdown `replace = true` works.
- Markdown `disable = true` works.
- Markdown structured bash/path permissions work.
- Markdown `runtime_kind` works or is diagnosed.

If split:

- Unsupported Markdown keys produce warnings.
- Docs clearly state TOML is required for full structured control.

### Acceptance criteria

- Markdown behavior is not surprising.
- Ignored frontmatter keys are either supported or diagnosed.
- README/examples match actual parser behavior.

## Phase 6: TUI command correctness

### Problem

TUI commands exist, but stateful commands are easy to partially implement. `/agents reload` and `/agent <name>` need to update the effective runtime state, not just display data.

### Required changes

Audit and harden:

- `/agents`: visible list grouping primary/all versus subagent/all.
- `/agents --all`: includes hidden/system and disabled agents, clearly marked.
- `/agents show <name>`: includes source stack, runtime kind, model source/resolution, permissions, diagnostics, prompt source/preview.
- `/agents diff <name>`: compares resolved final agent against built-in base where available and clearly handles custom-only agents.
- `/agents validate`: prints all registry diagnostics and returns a failure status in headless/CLI contexts if errors exist.
- `/agents reload`: rebuilds registry, refreshes agent state, preserves current agent if valid, otherwise falls back to configured default/build.
- `/agent <name>`: validates primary/all mode, rejects subagent-only and disabled/hidden agents where appropriate, and updates the session/core state used by future turns.

### Tests

- `/agent build` changes the selected active agent.
- `/agent security-review` fails because it is subagent-only.
- `/agent research` succeeds because it is all-mode.
- `/agents reload` picks up a new project agent in a temp project.
- `/agents reload` falls back safely if current agent becomes disabled.
- `/agents diff build` shows project overlay changes.
- `/agents diff custom-only` reports no built-in base.
- Hidden agents are excluded from default list and included in `--all`.

### Acceptance criteria

- TUI commands operate on the same registry/execution-profile path as runtime execution.
- Reload actually affects future selections/spawns.
- Mode constraints are enforced consistently between TUI selection, mention, and task tool.

## Phase 7: Generated built-in pipeline hardening

### Problem

Generated built-ins are a critical maintenance path. The generator should remain deterministic, strict for built-ins, and lightweight.

### Required changes

Tighten generator/check scripts:

- Ensure all generated files have stable formatting and sorted permissions.
- Fail generation on unknown built-in keys.
- Fail generation on duplicate names.
- Fail generation on missing prompt files when `prompt_file` is set.
- Fail generation on invalid runtime kind.
- Check generated Rust contains all fields added to `Agent`, including `fallback_model` and `runtime_kind`.
- Add a generated metadata summary test if helpful.

Add a local validation script if not already present:

```bash
python3 scripts/generate_builtin_agents.py --check
python3 scripts/check_builtin_agents.py
cargo test builtin_agent
```

### Tests

- Stale generated output fails check mode.
- Invalid built-in runtime kind fails check mode.
- Unknown built-in key fails check mode.
- Duplicate built-in name fails check mode.
- Permission order is deterministic.

### Acceptance criteria

- Built-in generation is stable and CI-enforced.
- Adding fields to `Agent` cannot silently leave generated built-ins incomplete.

## Phase 8: Documentation correction and examples audit

### Problem

Docs were added quickly across README, architecture docs, AGENTS.md, and examples. They should be reconciled against actual parser behavior.

### Required changes

Audit and align:

- README agent customization section.
- `assets/agents/README.md`.
- `examples/agents/README.md`.
- `architecture/agent.md`.
- `.codegg/skills/agent-loop/SKILL.md`.
- Example TOML/Markdown files.

Correct any mismatches around:

- Case sensitivity of modes: `Primary`/`Subagent`/`All` versus lowercase.
- `[permission]` versus `[agent.permissions]` format.
- `[bash_permission]` versus `[agent.bash_permission]` format.
- `[path_permission]` versus `[agent.path_permission]` format.
- Markdown frontmatter capabilities.
- Whether `/agent <subagent-only>` is allowed.
- Whether `/agents reload` is implemented as true reload or display-only.
- Whether `/agents create` exists or remains future work.

### Acceptance criteria

- Every documented example parses in tests.
- README examples match actual accepted syntax.
- `/agents validate` examples reflect real diagnostics.
- Future-work items are not documented as complete commands unless they work.

## Suggested execution order

1. Diagnostics and docs audit first, because they reveal parser/runtime mismatches quickly.
2. `AgentSpec` explicit merge refactor next, because it may change tests and command output.
3. Safety-envelope hot-path wiring next, because it is the main security/correctness requirement.
4. Model/runtime execution verification next.
5. TUI reload/selection correctness next.
6. Generator/CI hardening last, after field shapes settle.

## Final acceptance checklist

The pass is complete when all of the following are true:

- Built-ins remain compiled and generated from maintainer TOML/Markdown assets.
- Custom overlays preserve explicit default values through `AgentSpec`-style merging.
- Invalid custom files produce source-aware diagnostics.
- Missing prompt files are visible in `/agents validate`.
- User/project agents cannot escalate beyond session/config/hard safety policy.
- Subagent execution never emits an accidental empty model request.
- Markdown behavior is either equivalent to TOML or explicitly documented as narrower.
- TUI commands reflect and modify real runtime agent state.
- All documented examples parse and have tests.
- CI validates generated built-ins and catches stale output.

## Handoff note

Prefer narrow corrective commits with tests over broad rewrites. The feature already has most of the desired shape. This pass should make the semantics dependable and auditable before further expanding security/research runtime behavior.
