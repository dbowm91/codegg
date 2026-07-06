# Shell Output Projection Validation Polish Plan

## Objective

Close the remaining validation and operational polish gaps in the shell-output projection stack after the post-Phase 10 corrective pass. The implementation is now structurally complete through RTK invocation, expansion UX, deterministic redaction, evaluation fixtures, and context-budget metadata. The remaining work is to make the behavior demonstrably reliable in CI and to tighten the few places where RTK invocation still relies on permissive fallback behavior.

This pass should not add new feature scope. It should make the current feature set easier to trust, easier to validate, and harder to accidentally misuse.

## Current State

The current codebase appears to have:

- RTK capability probing for both stdin post-process and wrapped-command modes.
- `RtkInvocationMode` selection that prefers post-process, falls back to wrapper, and disables otherwise.
- Wrapper mode that prefers `CommandRun.argv` and propagates cwd.
- Redaction tests for false positives, long lines, multi-credential lines, PEM distinctions, and embedded credential URLs.
- Expansion boundary tests and handle round-trip tests.
- Compaction metadata tests that preserve warnings, redaction facts, and `is_already_projected`.

Remaining concerns:

1. Hosted CI visibility is absent or not discoverable from GitHub status checks.
2. RTK post-process probing is still a heuristic: non-empty stdout does not prove semantic compression of stdin.
3. Wrapper fallback still uses `split_whitespace()` when `argv` is missing.
4. Documentation still mixes historical phase notes with current behavior, which may confuse future agents.
5. Optional RTK real-binary validation is not clearly exposed as a maintained command.

## Workstream 1: CI Visibility and Harness Coverage

### Goal

Ensure the new projection, redaction, expansion, and compaction harnesses run in the standard validation path and are visible to maintainers.

### Tasks

1. Inspect existing GitHub Actions workflows and local validation scripts.
2. Confirm the normal validation path includes:
   - shell module unit tests
   - `tests/shell_projection_harness.rs`
   - `tests/shell_projection_phase10.rs`
   - redaction tests in `src/shell/redactor.rs`
   - RTK capability tests that do not require RTK installed
3. If CI does not run `cargo test --all-features`, add or adjust a job that does.
4. If full `cargo test --all-features` is too expensive, add a named projection validation job that runs the relevant tests explicitly.
5. Ensure GitHub status checks are associated with pushes/PRs to `main` or the repo's active integration branch.
6. Document the validation command set in `architecture/human_shell.md` and `.codegg/skills/human_shell/SKILL.md`.

### Acceptance Criteria

- CI status is visible on commits or PRs.
- The projection harness and Phase 10 tests are run by CI or a clearly documented validation workflow.
- RTK-absent CI environments pass cleanly.
- Optional RTK tests are not required for standard CI.

## Workstream 2: Stronger Optional RTK Contract Tests

### Goal

Add an optional real-binary RTK validation path that proves the detected RTK mode performs the expected semantic operation, not just that the binary exits successfully.

### Tasks

1. Add an ignored or env-gated integration test module, for example:

```bash
CODEGG_RTK_INTEGRATION=1 cargo test --all-features rtk_integration
```

2. Use a synthetic noisy input sample with repeated lines, success lines, warning lines, and error-like lines.
3. For post-process mode, feed the sample through the exact code path used by `RtkProjector::project_post_process()`.
4. Assert:
   - RTK was found and version captured.
   - The selected invocation mode is deterministic.
   - Output is non-empty.
   - Output is not exactly the unmodified input unless RTK explicitly documents pass-through behavior.
   - Projection metadata reports `ExternalCompressed` and `Lossy`.
   - Raw expansion handles still point to the retained original output.
5. For wrapper mode, run only a simple read-only command in a temp directory and assert cwd behavior.
6. Skip cleanly when RTK is unavailable or when the env flag is not set.

### Acceptance Criteria

- Maintainers have a documented command to validate RTK against a real installed binary.
- The optional test proves more than process availability.
- Test failures clearly identify whether the issue is discovery, post-process contract, wrapper contract, timeout, or projection metadata.

## Workstream 3: Tighten Wrapper Fallback Without `argv`

### Goal

Prevent wrapper mode from using loose whitespace parsing on complex shell strings.

### Tasks

1. Add a helper such as:

```rust
fn parse_simple_wrapper_command(command: &str) -> Result<Vec<String>, WrapperParseError>
```

2. Accept only a narrow grammar when `CommandRun.argv` is absent:
   - ASCII/simple command tokens separated by whitespace
   - no quotes
   - no backslashes
   - no shell metacharacters
   - no pipes
   - no redirects
   - no command substitution
   - no variable expansion
   - no semicolon
   - no boolean command chaining
   - no newlines
3. Reject commands containing:
   - `'`, `"`, `\`
   - `|`, `>`, `<`, `;`, `&`
   - `$`, `*`, `?`, `[`, `]`, `{`, `}`
   - backticks
   - leading env assignments
4. On rejection, return `ProjectionError::BackendUnavailable` or `ProjectionError::Unsupported` and let the selector fall back to safe projection.
5. Add tests for:
   - simple accepted commands
   - quoted path rejected without argv
   - path with spaces rejected without argv
   - pipeline rejected
   - redirect rejected
   - env assignment rejected
   - command substitution rejected
   - argv path accepts a path containing spaces because it is already tokenized

### Acceptance Criteria

- Wrapper mode without argv is limited to simple commands.
- Complex shell syntax never reaches `rtk <args>` through loose parsing.
- Existing safe fallback behavior remains intact.

## Workstream 4: Make RTK Raw-Handle Semantics Explicit in Types

### Goal

Move wrapper/raw semantics from a warning string into structured metadata so compaction, TUI, and model-facing projections can treat it reliably.

### Tasks

1. Add an enum such as:

```rust
pub enum ProjectionRawSemantics {
    OriginalCommandRaw,
    WrappedCommandRaw,
    OriginalRawUnavailable,
    Unknown,
}
```

2. Add the field to `ProjectionResult` or `ProjectionContextMetadata`, whichever is less invasive.
3. Set it explicitly:
   - native/generic projectors: `OriginalCommandRaw`
   - RTK post-process: `OriginalCommandRaw`
   - RTK wrapper: `OriginalCommandRaw` only if the original output was retained before wrapper projection; otherwise `WrappedCommandRaw` or `OriginalRawUnavailable`
4. Teach compaction metadata to preserve this field.
5. Render a concise TUI/model metadata line when semantics are not ordinary original raw.

### Acceptance Criteria

- Wrapper mode does not rely only on prose warnings for raw-handle truthfulness.
- Compaction preserves raw semantics.
- Tests cover RTK post-process and wrapper raw semantics.

## Workstream 5: Documentation Status Cleanup

### Goal

Make docs easy for future agents to interpret by separating current behavior from historical phase notes.

### Tasks

1. In `architecture/human_shell.md`, add a top-level “Current Shell Projection Behavior” section before the phase history.
2. Summarize current behavior in present tense:
   - raw output retention
   - projection selector
   - native projectors
   - config modes
   - RTK optional behavior
   - expansion UX
   - redaction
   - evaluation harness
   - context metadata
3. Move phase-by-phase notes under a “Historical Roadmap Status” heading.
4. Update `.codegg/skills/human_shell/SKILL.md` similarly because skills are operational guidance for future agents.
5. Keep `AGENTS.md` concise; point to architecture docs for detail.

### Acceptance Criteria

- A future agent can quickly determine current behavior without parsing the phase history.
- Phase 5 skeleton notes do not appear to contradict Phase 6 implementation.
- Optional RTK readiness is accurately described.

## Workstream 6: Validation Command Documentation

### Goal

Make validation repeatable.

### Tasks

Add a short validation block to the architecture doc and skill guide:

```bash
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
scripts/check-core-boundary.sh
```

Add optional RTK validation:

```bash
CODEGG_RTK_INTEGRATION=1 cargo test --all-features rtk_integration
```

If the optional test name differs, document the actual test filter.

### Acceptance Criteria

- Standard validation commands are documented in exactly one canonical place and referenced elsewhere.
- Optional RTK validation is clearly marked optional and skipped by default.

## Suggested Implementation Order

1. Inspect CI workflows and validation scripts.
2. Add/adjust CI or documented projection validation command.
3. Add env-gated RTK integration tests.
4. Tighten wrapper fallback parsing.
5. Add structured raw-semantics metadata if feasible.
6. Clean docs into current behavior vs roadmap history.
7. Run standard validation.
8. Run optional RTK validation only if RTK is installed.

## Tests to Add or Update

- `rtk_integration_post_process_contract_skips_without_env`
- `rtk_integration_post_process_contract_with_real_binary`
- `rtk_integration_wrapper_contract_with_real_binary`
- `wrapper_without_argv_accepts_simple_tokens`
- `wrapper_without_argv_rejects_quotes`
- `wrapper_without_argv_rejects_pipes_redirects_and_shell_expansion`
- `wrapper_with_argv_accepts_spaces_in_path`
- `projection_raw_semantics_native_is_original`
- `projection_raw_semantics_rtk_post_process_is_original`
- `projection_raw_semantics_rtk_wrapper_is_explicit`
- `compaction_preserves_raw_semantics`

## Success Criteria

- CI visibility is fixed or the absence is explicitly documented with the expected local validation path.
- The projection harness and context metadata tests run under standard validation.
- Optional RTK tests validate actual RTK behavior when enabled.
- Wrapper mode cannot loosely parse complex shell strings without argv.
- Raw-handle semantics are structured or otherwise strongly test-covered.
- Documentation clearly distinguishes current behavior from historical phase status.

## Non-Goals

- Do not expand native projectors to more languages.
- Do not make RTK default.
- Do not bundle RTK.
- Do not add model-generated command summaries.
- Do not redesign the shell execution subsystem.

## Handoff Notes

Keep this pass intentionally small. The projection stack is already feature-rich; the goal is to make validation reliable and edge-case behavior explicit. Favor safe fallback over clever RTK invocation whenever a command shape or RTK contract is ambiguous.
