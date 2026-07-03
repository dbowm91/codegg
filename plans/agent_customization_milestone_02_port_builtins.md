# Agent Customization Milestone 2: Port Current Built-ins to Generated Rust

## Goal

Move the current hardcoded built-in agents from Rust source into maintainer-facing TOML/Markdown assets, generate checked-in Rust from those assets, and preserve existing behavior.

This milestone is a migration, not a redesign. The public `builtin_agents() -> Vec<Agent>` behavior should remain compatible with the current system.

## Built-ins to port

Port all existing built-ins:

```text
build
plan
general
explore
title
summary
compaction
security-review
research
```

Each built-in should have one TOML asset under `assets/agents/` and one prompt Markdown asset under `assets/prompts/agents/` unless the prompt is intentionally empty.

## Migration strategy

1. Copy the current Rust values into TOML assets.
2. Move large inline prompt strings into Markdown files.
3. Run the generator.
4. Replace the hardcoded constructor list with the generated constructor list.
5. Keep the existing public API stable.
6. Add tests for behavioral invariants.

The generated code may still construct the existing `Agent` type directly. Introducing `AgentSpec`/`ResolvedAgent` is deferred to the registry milestone.

## Expected module shape

```text
src/agent/
  mod.rs
  builtins/
    mod.rs
    generated.rs
```

`src/agent/builtins/mod.rs`:

```rust
mod generated;
pub use generated::builtin_agents;
```

`src/agent/mod.rs` should no longer contain a large hand-written `builtin_agents()` constructor. It should either re-export from `builtins` or call into it.

## Asset contents

Each TOML should describe only structured metadata and permissions. Prompt text should live in Markdown. Example:

```toml
schema_version = 1
name = "research"
role = "researcher"
description = "Long-horizon research agent using structured research and web tools."
mode = "all"
hidden = false
color = "magenta"
temperature = 0.2
steps = 24
prompt_file = "prompts/agents/research.md"

[permission]
read = "allow"
glob = "allow"
grep = "allow"
list = "allow"
websearch = "allow"
webfetch = "allow"
research = "allow"
skill = "allow"
question = "allow"
task = "allow"
bash = "ask"
edit = "ask"
write = "ask"
apply_patch = "ask"
multiedit = "ask"
terminal = "ask"
commit = "ask"
image = "deny"
plan_enter = "deny"
plan_exit = "deny"
```

## Behavioral invariants

Add tests that assert important properties rather than full prompt snapshots:

- Built-in count remains 9.
- `build` is available as a visible primary agent.
- `plan` is visible and denies mutating tools.
- `general` and `explore` remain subagents.
- `title`, `summary`, and `compaction` remain hidden.
- `compaction` denies all or remains equivalently locked down.
- `security-review` remains a subagent and allows read/search/security/LSP while denying mutation.
- `research` remains mode `all`, allows research/web tools, and denies image/plan controls.

Prompt tests should assert key sentinel phrases for specialized agents:

- `security-review` prompt mentions defensive review, deterministic checks, evidence, and no exploit/offensive automation.
- `research` prompt mentions the research tool, websearch/webfetch distinction, and source/citation expectations.

## Risk areas

String escaping in generated Rust can accidentally alter prompt text. Add a test that loads the generated `security-review` and `research` prompts and checks representative substrings.

Permission maps can lose entries during conversion. Add metadata tests for all current permissions on the specialized agents.

Generated file churn can make reviews noisy. Sort TOML files and permission keys deterministically.

## Acceptance criteria

- All current built-ins are represented in `assets/agents/*.toml`.
- All large prompts are represented in `assets/prompts/agents/*.md`.
- `src/agent/builtins/generated.rs` contains generated constructors for all built-ins.
- The old hardcoded built-in constructor list is removed or reduced to a generated re-export.
- Existing agent tests pass.
- New invariant tests cover count, mode, hidden state, specialized permissions, and key prompt content.
- Runtime does not read built-in TOML or prompt files.

## Handoff notes

Do not combine this migration with overlay semantics. The objective is to prove that generated Rust can replace the hardcoded values without functional drift. Once that is stable, the registry and customization work can build on it.
