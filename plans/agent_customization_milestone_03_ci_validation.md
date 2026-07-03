# Agent Customization Milestone 3: CI and Built-in Validation Hardening

## Goal

Make generated built-ins safe to maintain by adding stale-output checks, schema checks, deterministic generation checks, and runtime invariant tests.

This milestone should catch accidental changes to built-in agent behavior before they land.

## Scope

Add CI coverage for:

```text
python3 scripts/generate_builtin_agents.py --check
cargo test builtin_agents
cargo test agent_assets
```

Exact test names can differ, but the coverage should exist.

## Generator check mode

`--check` should:

1. Read all built-in TOML and prompt assets.
2. Generate Rust to an in-memory string or temporary file.
3. Compare against checked-in `src/agent/builtins/generated.rs`.
4. Exit non-zero on drift.
5. Print a concise remediation message:

```text
src/agent/builtins/generated.rs is stale.
Run: python3 scripts/generate_builtin_agents.py
```

Do not auto-update files during CI check mode.

## Asset validation rules

Built-in asset validation should fail generation for:

- Missing `schema_version`.
- Unsupported `schema_version`.
- Missing `name`.
- Duplicate `name`.
- Invalid `mode`.
- Missing `description` for visible agents.
- Missing prompt file when `prompt_file` is set.
- Prompt file outside `assets/`.
- Invalid permission action outside `allow`, `ask`, `deny`.
- Unknown top-level keys unless explicitly allowed.

Unknown keys should fail for built-ins. User/project files can be more forgiving later, but built-ins should be strict.

## Runtime invariant tests

Add tests for each built-in class.

Primary/all agents:

- `build` is visible and selectable.
- `plan` is visible and selectable.
- `research` is visible and selectable if mode remains `all`.

Subagents:

- `general` is spawnable.
- `explore` is spawnable and read-oriented.
- `security-review` is spawnable and defensive.

Hidden system agents:

- `title` is hidden.
- `summary` is hidden.
- `compaction` is hidden.

Permission invariants:

- `security-review` denies mutating tools.
- `security-review` allows `security` and `lsp`.
- `research` allows `research`, `websearch`, and `webfetch`.
- `compaction` has the most restrictive permission set.

Prompt invariants:

- `security-review` includes defensive-only language.
- `security-review` includes evidence/finding distinction.
- `research` includes research tool guidance.
- `research` includes source/citation guidance.

## Determinism tests

Add a generator unit test or script mode to verify deterministic output:

```text
generate once -> buffer A
generate again -> buffer B
assert A == B
```

If this is awkward in Python, the check-mode comparison plus sorted traversal is sufficient, but deterministic sorting must be explicit in the implementation.

## CI integration

Add the generator check to the existing CI flow without making ordinary Cargo build depend on Python. The CI command can be in a script or workflow step. If CI images lack Python 3.11, document and install it in the workflow rather than weakening the generator.

Recommended CI order:

```text
python3 --version
python3 scripts/generate_builtin_agents.py --check
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Use the repo's existing CI conventions if they differ.

## Documentation updates

Add a maintainer note near the generator or in docs:

```text
Built-in agents are edited in assets/agents and assets/prompts.
After editing, run scripts/generate_builtin_agents.py and commit the generated Rust.
Normal users do not need these assets at runtime.
```

## Acceptance criteria

- CI fails when generated Rust is stale.
- Built-in asset schema mistakes fail generation.
- Runtime tests cover built-in count, mode, hidden state, permissions, and specialized prompts.
- Generated Rust remains committed.
- No normal runtime or build path requires built-in TOML/prompt file reads.

## Handoff notes

Be strict for built-ins. A broken built-in is a release-blocking error, not a runtime warning. Later milestones can treat user/project files with softer diagnostics.
