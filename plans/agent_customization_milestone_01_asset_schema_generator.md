# Agent Customization Milestone 1: Built-in Asset Schema and Generator

## Goal

Create the maintainer-facing source layout and generation tooling for compiled built-in agents. This milestone should not change runtime behavior. It establishes the source asset format, generator contract, and checked-in generated Rust location.

## Scope

Add source assets for built-in agents and prompts:

```text
assets/agents/*.toml
assets/prompts/agents/*.md
assets/prompts/contracts/*.md
```

Add generated Rust destination:

```text
src/agent/builtins/generated.rs
src/agent/builtins/mod.rs
```

Add maintainer scripts:

```text
scripts/generate_builtin_agents.py
scripts/check_builtin_agents.py
```

The generator should output Rust code that constructs the existing `Agent` values. It should not emit runtime logic beyond simple constructors and static helper data.

## Design requirements

Built-ins must remain compiled into the binary. Runtime should not read `assets/agents/*.toml` or `assets/prompts/**/*.md`.

Normal `cargo build` must not require Python. Generated Rust is checked into source control.

The Python script should use deterministic file ordering and stable formatting. Two runs over the same inputs must produce byte-identical `generated.rs`.

The script should use only the Python standard library. Prefer Python 3.11+ `tomllib` unless the project explicitly supports older Python. If older Python support is desired, add a clear error telling maintainers to use Python 3.11+ rather than adding a PyPI dependency.

Rust remains the authority for runtime semantics. The generator should not duplicate permission-evaluation logic, model routing logic, prompt layering logic, or task/subagent dispatch logic.

## Initial schema

Define the first asset schema to match existing `Agent` fields as closely as possible:

```toml
schema_version = 1
name = "security-review"
role = "security_reviewer"
description = "Defensive security review of changed code using deterministic scanning and semantic evidence."
mode = "subagent" # primary | subagent | all
hidden = false
color = "red"
temperature = 0.1
top_p = 0.9
steps = 16
model = ""
variant = ""
reasoning_effort = ""
thinking_budget = 0
prompt_file = "prompts/agents/security-review.md"

[permission]
read = "allow"
grep = "allow"
glob = "allow"
list = "allow"
security = "allow"
lsp = "allow"
bash = "ask"
edit = "deny"
write = "deny"
apply_patch = "deny"
replace = "deny"
multiedit = "deny"
commit = "deny"
terminal = "deny"
```

Avoid over-modeling in this milestone. More expressive permissions and runtime kinds come later.

## Generator behavior

The generator should:

1. Find `assets/agents/*.toml`.
2. Sort files lexicographically by normalized path.
3. Parse each TOML file.
4. Validate `schema_version`, `name`, `mode`, and `prompt_file` if present.
5. Resolve `prompt_file` relative to `assets/`.
6. Escape strings safely for Rust raw string literals.
7. Emit `src/agent/builtins/generated.rs`.
8. Include a generated-file header.
9. Provide `--check` mode that fails if the checked-in file is stale.

The generated file should expose:

```rust
pub fn builtin_agents() -> Vec<Agent> { ... }
```

and should be re-exported by:

```rust
// src/agent/builtins/mod.rs
mod generated;
pub use generated::builtin_agents;
```

## Rust integration

Do not remove the current hardcoded built-ins yet unless this milestone is combined with Milestone 2. Initially, add the generated module behind a test-only or unused path to allow review of the generated output.

If integrating immediately, preserve the public function name so existing callers do not change:

```rust
pub use builtins::builtin_agents;
```

## Validation

Add tests or script checks for:

- Duplicate agent names fail generation.
- Missing `name` fails generation.
- Invalid `mode` fails generation.
- Missing `prompt_file` target fails generation.
- Output ordering is deterministic.
- Generated file contains the `@generated` header.

## Acceptance criteria

- `assets/agents/` and `assets/prompts/` exist with at least one pilot built-in asset.
- `scripts/generate_builtin_agents.py` generates deterministic Rust.
- `scripts/check_builtin_agents.py` or `generate_builtin_agents.py --check` detects stale output.
- `src/agent/builtins/generated.rs` is checked in.
- Normal `cargo build` does not need Python.
- No runtime file reads are introduced for built-in agents.

## Handoff notes

Keep the first generator intentionally boring. It is acceptable if the generated Rust is verbose. The important property is that the runtime sees regular Rust constructors and users cannot break defaults by deleting files.
