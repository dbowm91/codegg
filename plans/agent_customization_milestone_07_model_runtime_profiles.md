# Agent Customization Milestone 7: Model Inheritance and Runtime Profiles

## Goal

Make agent model selection predictable and add explicit runtime kinds for special agents such as `security-review`, `research`, `compaction`, `title`, and `summary`.

This milestone should fix ambiguous subagent model behavior and make specialized agents first-class without allowing TOML to encode arbitrary runtime logic.

## Model inheritance

A custom or built-in agent may omit `model`. Omitted model must not lead to an empty model request or a hardcoded provider fallback unless no better option exists.

Recommended resolution order:

```text
1. explicit agent.model
2. role-specific default from config/model profile
3. runtime/session selected model
4. parent agent model, for subagent task calls
5. global config model
6. provider default, only with diagnostic
```

For subagents, parent/session inheritance should be explicit in logs/diagnostics.

## Model aliases

Support aliases compatible with the wider Codegg/eggpool direction:

```toml
model = "tier.frontier"
fallback_model = "tier.workhorse"
```

Alias resolution should happen through the existing model routing/profile path where possible. Agent registry should not hardcode provider-specific routing decisions.

Diagnostics should show both requested and resolved model:

```text
agent research: model tier.frontier resolved to eggpool/gpt-5.5
agent explore: model omitted, inheriting parent model eggpool/mimo
```

## Runtime kind field

Add optional runtime classification to agent specs:

```toml
[runtime]
kind = "security_review"
```

Supported values:

```text
standard
security_review
research
compaction
title
summary
```

Default is `standard`.

Generated built-ins should set explicit runtime kind for known special agents:

```text
security-review -> security_review
research -> research
compaction -> compaction
title -> title
summary -> summary
build/plan/general/explore -> standard
```

## Runtime behavior boundaries

TOML selects a Rust-defined runtime kind. It must not define executable behavior, arbitrary shell preflight, or custom tool chains.

Rust remains responsible for behavior such as:

- security preflight collection
- research tool orchestration
- compaction contracts
- title/summary constraints
- hidden/system agent handling

## Security-review runtime profile

Initial behavior can be metadata-only, preserving current prompting. Later behavior may add preflight collection:

```text
collect changed files/hunks
run security.run_profile(profile = "security_review")
request LSP securityContext for changed areas
include deterministic findings/risk markers in the subagent context
require findings to cite evidence
```

The security runtime profile should remain defensive-only. It should not generate exploit chains, payloads, or offensive automation.

## Research runtime profile

Initial behavior can preserve current prompting. Later behavior may add structured orchestration:

```text
if task is multi-hop or asks for external current/niche facts, prefer research tool
use websearch/webfetch for quick lookup
require citation-bearing synthesis when external sources are used
summarize uncertainty and source quality
```

The research runtime should integrate with `ResearchService` rather than only relying on prompt instructions.

## Subagent task integration

When spawning a subagent, task execution should receive a fully resolved runtime profile:

```rust
pub struct ResolvedAgentExecutionProfile {
    pub agent: Agent,
    pub runtime_kind: AgentRuntimeKind,
    pub resolved_model: String,
    pub effective_permissions: PermissionRuleset,
}
```

Exact type names can differ. The important requirement is that task execution should not re-derive provider/model behavior from raw strings in an ad hoc way.

## Tests

Add model tests:

- Explicit agent model wins.
- Agent without model inherits parent/session model.
- Global config model applies when no parent/session model exists.
- Empty model requests are not emitted for normal subagent execution.
- Provider default fallback emits a diagnostic.

Add runtime kind tests:

- Built-in special agents have expected runtime kinds.
- Unknown runtime kind produces a diagnostic for user/project files.
- Unknown runtime kind fails built-in generation.
- Standard agents default to `standard`.

Add task integration tests if feasible:

- Spawning `research` uses resolved model.
- Spawning `security-review` uses resolved model and security runtime kind.
- Primary-only agents cannot be spawned as subagents unless mode is `all`.

## Acceptance criteria

- Subagent execution no longer depends on empty model strings or hardcoded provider fallback except as last resort.
- Runtime kind exists and is represented on resolved agents.
- Built-in special agents declare runtime kind through generated assets.
- Unknown runtime kinds are handled safely.
- Security/research remain Rust-defined runtime profiles, not TOML-executable workflows.

## Handoff notes

Do not overbuild the specialized preflight in this milestone if it risks destabilizing task execution. The minimum useful result is model inheritance correctness plus explicit runtime kind metadata. Specialized security/research behavior can deepen incrementally after the profile hooks exist.
