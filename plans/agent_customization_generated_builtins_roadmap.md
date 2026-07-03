# Agent Customization and Generated Built-ins Roadmap

## Purpose

Move Codegg's core agents, subagents, and prompts to a maintainable source-of-truth model without weakening runtime reliability. Built-in agents should remain compiled Rust defaults so normal `cargo build` and installed binaries do not depend on runtime TOML files, prompt files, Python, or file-system assets. Maintainers should edit declarative TOML/Markdown assets and regenerate checked-in Rust. Users and projects should be able to customize, override, or add agents through TOML/Markdown overlays in global and project config directories.

This roadmap intentionally separates maintainer-facing built-in asset generation from user-facing customization. Built-ins are compiled and immutable by default. User/project agents are optional overlays.

## Current-state assumptions

Codegg already has the right primitives to support this direction:

- Hardcoded built-in agents exist in `src/agent/mod.rs`.
- Built-ins include primary agents, subagents, hidden system agents, `security-review`, and `research`.
- Existing `Agent` fields cover role, mode, model, variant, temperature/top-p, prompt, description, color, steps, hidden state, permissions, thinking budget, and reasoning effort.
- Current resolution starts with built-ins, then applies global/project agent files and config-level `agent`/`mode` overrides.
- The task/subagent path, permission checker, research tool, security tool, and LSP security context already provide most of the runtime substrate for specialized agents.

The missing work is consolidation: a generated built-in source pipeline, a real agent registry, safe overlay semantics, richer per-agent permissions, model inheritance, diagnostics, and TUI commands.

## Non-goals

This roadmap does not make built-in agents runtime-loaded from TOML files. Built-ins must still compile into Rust.

This roadmap does not add arbitrary scriptable agent behavior. TOML should describe agent metadata, prompts, permissions, runtime kind, and model preferences. Rust must remain the authority for runtime behavior.

This roadmap does not require a build-time Python dependency for normal users. Python generation is a maintainer operation and CI check. Generated Rust should be committed.

## Target architecture

Source assets for maintainers:

```text
assets/
  agents/
    build.toml
    plan.toml
    general.toml
    explore.toml
    security-review.toml
    research.toml
    title.toml
    summary.toml
    compaction.toml
  prompts/
    agents/
      build.md
      plan.md
      general.md
      explore.md
      security-review.md
      research.md
      title.md
      summary.md
      compaction.md
    contracts/
      base_harness.md
      websearch.md
      research_subagent.md
      security_review_output.md
      research_output.md
```

Generated checked-in Rust:

```text
src/agent/builtins/
  mod.rs
  generated.rs
```

User/project overlays:

```text
~/.config/codegg/agents/*.toml
~/.config/codegg/agents/*.md
.codegg/agents/*.toml
.codegg/agents/*.md
```

Runtime resolution:

```text
compiled generated built-ins
  -> global user agent overlays
  -> project agent overlays
  -> config.agent overrides
  -> config.mode compatibility overrides
  -> session/runtime safety envelope
```

## Design principles

Built-ins are safe defaults. Users should not be able to break default agents by deleting files from a config directory or installed package.

Generated Rust is checked in. `cargo build` should not require Python or asset generation.

Python generation is narrow. It should scan assets, resolve prompt files, validate basic required fields, and emit deterministic Rust. Rust remains responsible for agent semantics.

Overlays merge by default. A user should be able to set `model = "tier.frontier"` for `security-review` without accidentally deleting the built-in prompt and deny rules.

Diagnostics must be source-aware. `/agents diff security-review` should explain which layers contributed to the final resolved agent and which fields changed.

Security and research should be first-class runtime profiles, not just prompts. Their TOML selects a Rust-defined runtime kind; it must not embed executable logic.

## Milestones

### Milestone 1: Built-in asset schema and directory layout

Create maintainer-facing asset directories and define the initial `BuiltinAgentSpec` schema. The first schema should map closely to the existing `Agent` struct to minimize behavior risk.

### Milestone 2: Generator and checked-in Rust output

Add `scripts/generate_builtin_agents.py`, `scripts/check_builtin_agents.py`, and generated Rust under `src/agent/builtins/generated.rs`. The generated output should expose `builtin_agents() -> Vec<Agent>`.

### Milestone 3: Port hardcoded built-ins to generated Rust

Move all current hardcoded built-ins into TOML/Markdown source assets, generate Rust, preserve behavior, and delete the hardcoded constructor list from `src/agent/mod.rs`.

### Milestone 4: CI and validation hardening

Add stale-generation checks, built-in parse/resolve tests, permission invariants, prompt-file validation, and generated output determinism checks.

### Milestone 5: AgentSpec, ResolvedAgent, and AgentRegistry

Separate declaration from resolved runtime profile. Centralize agent loading, merging, validation, lookup, source stacks, and diagnostics in an `AgentRegistry`.

### Milestone 6: User and project custom agents

Support TOML and Markdown custom agents in global and project directories. Fix Markdown body-as-prompt behavior and make TOML the canonical structured format.

### Milestone 7: Safe overlay and permission semantics

Implement merge-by-default overrides, explicit replacement, disabling, richer per-agent permission rules, bash/path pattern conversion, and safety-envelope intersection.

### Milestone 8: Model inheritance and routing aliases

Repair subagent model inheritance, add deterministic resolution diagnostics, and support model tier aliases compatible with Codegg's router/eggpool direction.

### Milestone 9: Specialized security and research runtime profiles

Add `runtime.kind` and use Rust-defined behavior for `security_review`, `research`, `compaction`, `title`, and `summary`. Security/research should gain optional preflight behavior over time.

### Milestone 10: TUI/CLI UX and documentation polish

Add `/agents`, `/agents show`, `/agents diff`, `/agents validate`, `/agents reload`, `/agents create`, and `/agent <name>`. Document built-in asset generation and user/project overlays.

## File naming convention

Detailed handoff plans for this roadmap live under:

```text
plans/agent_customization_milestone_01_asset_schema_generator.md
plans/agent_customization_milestone_02_port_builtins.md
plans/agent_customization_milestone_03_ci_validation.md
plans/agent_customization_milestone_04_registry_resolution.md
plans/agent_customization_milestone_05_user_project_agents.md
plans/agent_customization_milestone_06_overlay_permissions.md
plans/agent_customization_milestone_07_model_runtime_profiles.md
plans/agent_customization_milestone_08_tui_docs_polish.md
```

The milestone files group related low-level milestones into implementation-sized handoff plans.

## Validation strategy

At each step, maintain current behavior unless the milestone explicitly changes semantics. Add tests around resolved metadata rather than full prompt snapshots. The key invariants are:

- Built-in count remains stable until intentionally changed.
- Hidden agents stay hidden.
- `security-review` remains defensive, read-oriented, and mutation-denied.
- `research` keeps research/web capabilities and citation-oriented prompting.
- User overlays cannot silently escalate beyond the active session safety envelope.
- Missing or invalid built-in assets fail generation or CI.
- Missing or invalid user/project assets produce diagnostics, not panics.

## Rollout strategy

Implement generated built-ins first with compatibility wrappers, then introduce `AgentRegistry`, then enable customization semantics. This keeps the risk profile low and avoids combining asset generation, runtime registry refactor, permission changes, model inheritance, and TUI UX in one large pass.
