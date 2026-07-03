# Agent Customization Final Polish Caveats

## Purpose

The generated built-in and custom agent system is now functionally complete and has received a correctness hardening pass. This final polish plan targets the remaining caveats before the feature should be considered stable: CI/status visibility, mode casing normalization, fallback model policy, docs/test reconciliation, and final runtime verification.

This is intentionally a small polish pass. Avoid expanding the feature surface unless doing so removes a confusing edge case.

## Current caveats

1. CI visibility could not be confirmed through commit status APIs, even though the hardening commit reports local/test success.
2. Built-in TOML and user TOML currently use different mode casing conventions.
3. Model resolution has a hardcoded fallback to `openai/gpt-4o` when no model is configured.
4. Markdown agent files are intentionally narrower than TOML; docs now state this, but examples/tests should prove the split.
5. The safety-envelope and execution-profile path should receive a final integration-oriented verification pass.
6. TUI docs and command behavior should be reconciled one final time after the hardening changes.

## Non-goals

Do not rework the generated built-in architecture.

Do not make built-in agents runtime-loaded from TOML.

Do not make Markdown equal to TOML unless the change is small and low-risk. The current prompt-first/merge-only Markdown model is acceptable if documented and tested.

Do not add advanced security/research preflight orchestration in this pass.

## Phase 1: CI and status visibility polish

### Problem

The hardening commit reports that tests pass, but GitHub status APIs may not expose workflow runs or combined statuses for the commit. This creates ambiguity during remote reviews.

### Tasks

1. Inspect `.github/workflows/*` and confirm the workflow triggers include direct pushes to the active branch and pull requests.
2. Confirm the new `agent-assets` job is present and not accidentally gated behind only pull-request triggers.
3. Ensure all required checks have clear job names:
   - `agent-assets`
   - `fmt`
   - `check`
   - `clippy`
   - `test`
   - any existing plugin/example jobs
4. Add a short local validation command block to the relevant developer docs:

```bash
python3 scripts/generate_builtin_agents.py --check
python3 scripts/check_builtin_agents.py
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

5. If the repo intentionally avoids GitHub status checks on direct push, document that local validation is the source of truth for handoff commits.

### Tests / verification

- Confirm workflow YAML parses.
- Confirm `agent-assets` runs before or alongside Rust checks.
- Confirm `scripts/generate_builtin_agents.py --check` and `scripts/check_builtin_agents.py` are included in either CI or documented local validation.

### Acceptance criteria

- Reviewers can determine which validation commands are expected.
- CI behavior is either visible through GitHub Actions or explicitly documented as local-only for direct pushes.
- No stale generated built-ins can pass the documented validation path.

## Phase 2: Normalize or soften mode casing

### Problem

Built-in TOML uses capitalized modes (`Primary`, `Subagent`, `All`) because the generator maps them directly to Rust enum variants. Runtime user TOML uses lowercase modes (`primary`, `subagent`, `all`) because it goes through `parse_mode()`. This is now documented, but it remains a friction point and a likely support issue.

### Preferred fix

Make user/project TOML parsing case-insensitive for mode values while preserving the built-in generator behavior.

Accept all of:

```text
primary, Primary, PRIMARY
subagent, Subagent, SUBAGENT
all, All, ALL
```

Normalize internally to `AgentMode`.

### Alternative fix

If parser normalization is too invasive, keep lowercase-only runtime TOML but add a diagnostic that is unusually explicit:

```text
invalid mode "Subagent"; user TOML expects lowercase "subagent". Built-in TOML uses capitalized mode because it is compiled by the generator.
```

### Tasks

1. Update `parse_mode()` or the TOML loader to normalize case for user/project files.
2. Update tests for all accepted spellings if normalization is implemented.
3. Update docs to remove the split-casing caveat if normalization lands.
4. Keep built-in generator validation strict or separately normalize built-in mode strings; choose one and document it.

### Acceptance criteria

- A copied built-in-style custom TOML file does not fail only because `mode = "Subagent"` is capitalized, or the error message clearly tells the user exactly what to change.
- README and examples match actual parser behavior.

## Phase 3: Replace hardcoded fallback model with policy

### Problem

The hardening pass ensures model resolution never returns an empty string by falling back to `openai/gpt-4o` when no model is configured. That is safer than an empty model request, but a hardcoded provider/model can be surprising in a multi-provider/BYO-model tool.

### Desired behavior

Model fallback should follow an explicit policy:

```text
agent.model
agent role default
parent/session model
config.model
config.default_model or model_profile default
provider/router default if available
final hardcoded emergency fallback only with warning diagnostic
```

The final hardcoded fallback should be rare, visible, and preferably centralized in one constant.

### Tasks

1. Locate all hardcoded fallback model strings added for agent execution.
2. Replace scattered fallback literals with a named constant or config accessor:

```rust
const EMERGENCY_DEFAULT_MODEL: &str = "openai/gpt-4o";
```

or, preferably:

```rust
ModelResolutionPolicy::emergency_default()
```

3. Emit a warning diagnostic whenever the emergency fallback is used.
4. Consider allowing config to override the emergency default:

```toml
[agent_model]
emergency_default = "eggpool/default"
```

or reuse existing model/profile configuration if already present.

5. Ensure TUI `/agents show <name>` and `/agents validate` can display when an agent would fall back to emergency default.

### Tests

- With explicit model, no fallback diagnostic.
- With parent/session model, no emergency fallback.
- With config default model, no emergency fallback.
- With no configured model at all, emergency fallback is used and diagnostic is emitted.
- No `ChatRequest` receives an empty model.

### Acceptance criteria

- Hardcoded fallback is centralized and documented.
- Emergency fallback usage is observable.
- Users relying on eggpool/BYO routing are not silently pushed to an unexpected provider unless no model policy exists.

## Phase 4: Final safety-envelope integration verification

### Problem

The hardening pass says the safety envelope is applied on all execution paths. A final audit should ensure no path still constructs a `PermissionChecker` or executable agent profile from partially resolved agent data.

### Tasks

Trace and verify these paths:

- Normal TUI turn with selected primary/all agent.
- `/agent <name>` selection.
- Agent selection dialog.
- `@mention` subagent spawn.
- `task` tool spawn.
- Auto security review spawn.
- Headless mode.
- Test harness helper paths.

For each path, document the exact function that produces the final effective permissions.

### Tests

Add or strengthen integration tests:

- Custom project agent allows `edit`, session denies `edit`; final decision denies.
- Custom project agent allows `bash`, config/session says ask; final decision asks.
- Custom project agent tries to allow hard-denied tool; final decision denies.
- Subagent task execution sees the same bounded permissions as registry inspection.
- `/agents show` effective permissions match execution profile decisions for representative tools.

### Acceptance criteria

- There is one clear execution-profile path or a documented equivalent for each execution route.
- No custom agent can escalate beyond session/config/hard policy.
- Tests cover both primary and subagent execution paths.

## Phase 5: Markdown scope tests and docs reconciliation

### Problem

Markdown is now deliberately prompt-first and merge-only. That is acceptable, but unsupported frontmatter keys must not be silently ignored in a way that looks like success.

### Tasks

1. Add tests that Markdown with unsupported keys emits warnings:
   - `replace`
   - `merge`
   - `bash_permission`
   - `path_permission`
2. Confirm Markdown supports the documented supported keys:
   - `name`
   - `mode`
   - `description`
   - `model`
   - `temperature`
   - `color`
   - `steps`
   - `hidden`
   - `prompt`
   - `prompt_file`
   - flat `permission`
   - `disable`, if intentionally supported
3. Confirm README and examples do not imply Markdown supports TOML-only features.
4. Add one example Markdown file that intentionally stays prompt-first and simple.

### Acceptance criteria

- Unsupported Markdown frontmatter keys are diagnosed.
- Docs and examples accurately describe Markdown as narrower than TOML.
- The Markdown user experience is simple rather than half-supported.

## Phase 6: TUI command final polish

### Problem

The TUI command surface exists and was hardened, but small behavior mismatches can still create user confusion.

### Tasks

1. Confirm `/agent <name>` accepts only primary/all non-hidden, non-disabled agents.
2. Confirm the error for `/agent security-review` says to use `@security-review` or the task/subagent path.
3. Confirm `/agents --all` marks hidden and disabled agents clearly.
4. Confirm `/agents diff <custom-only>` does not imply there is a missing built-in base.
5. Confirm `/agents diff <builtin>` shows overlay field changes and critical unchanged fields.
6. Confirm `/agents reload` actually changes future selections/spawns, not just the displayed list.
7. Confirm headless `/agents validate` returns non-zero or error status when error diagnostics exist.

### Tests

- Command parser tests for `/agents`, `/agents --all`, `/agents show`, `/agents diff`, `/agents validate`, `/agents reload`.
- Selection tests for build/research/security-review/custom agents.
- Reload test using temp `.codegg/agents` directory.

### Acceptance criteria

- TUI command behavior matches docs.
- Error messages direct the user toward the correct action.
- Reload affects actual runtime state.

## Phase 7: Example files parse test

### Problem

Example files often drift because they are not exercised by tests.

### Tasks

Add a test that loads every file under `examples/agents/` and verifies:

- TOML files parse.
- Markdown files parse.
- Example agents have names, descriptions, and modes.
- Permissions use valid actions.
- Structured permission examples convert into runtime permission rules.
- Prompt bodies are non-empty where expected.

### Acceptance criteria

- Every documented example is tested.
- Changing parser semantics breaks tests if examples are not updated.

## Suggested execution order

1. Mode casing normalization or explicit diagnostics.
2. Central fallback model policy.
3. Safety-envelope integration spot-check/tests.
4. Markdown unsupported-key diagnostics/tests.
5. TUI command polish tests.
6. Example parse tests.
7. CI/status documentation cleanup.

## Final acceptance checklist

This polish pass is done when:

- CI/local validation expectations are unambiguous.
- User TOML mode casing is either flexible or produces a precise diagnostic.
- Emergency fallback model is centralized, documented, and diagnostic-bearing.
- Safety envelope is tested on primary and subagent hot paths.
- Markdown limitations are tested, not only documented.
- TUI command behavior matches docs and has representative tests.
- All `examples/agents/*` files are parser-tested.

## Handoff note

Prefer surgical fixes. The current architecture is already strong. This pass should remove sharp edges, not introduce another broad agent-system refactor.
