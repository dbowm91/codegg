# Shell Output Projection Post-Phase 10 Corrective Pass

## Objective

Tighten the shell-output projection implementation after the cleanup pass and Phases 6-10. The repo now has a coherent projection stack: raw command artifacts, projector selection, native Git/Rust projectors, optional RTK paths, expansion UX, deterministic redaction, fixture-based evaluation, and context-budget metadata. The remaining risk is no longer missing architecture; it is correctness hardening around RTK invocation semantics, wrapper safety, validation coverage, and the integration boundaries between redaction, expansion, and compaction.

This corrective pass should be completed before considering RTK-backed compression production-ready or expanding native projectors to more ecosystems.

## Current State

The current implementation appears to include:

- Phase 1 command event model and raw output retention.
- Phase 2 projector trait, generic projectors, projection metadata, and selector.
- Phase 3 native Git/Rust projectors.
- Phase 4 config schema and TUI metadata, with per-command rules deferred.
- Phase 5 RTK discovery and skeleton, corrected so fake placeholder output is not normal runtime behavior.
- Phase 6 RTK invocation paths for low-risk commands.
- Phase 7 expansion handles and TUI UX.
- Phase 8 deterministic redaction pipeline.
- Phase 9 fixture-based evaluation harness.
- Phase 10 context-budget and compaction metadata.

The implementation is ahead of the original roadmap. The corrective work should therefore be targeted and conservative.

## Main Findings to Correct

### 1. RTK capability probing may never enable invocation modes

`RtkCapabilities::invocation_mode()` only returns `PostProcess` or `Wrapper` when `supports_post_process` or `supports_wrapper_mode` is explicitly `CapabilityState::Yes`. The current `probe_capabilities()` path appears to probe exit-code and UTF-8 behavior, but does not clearly set either `supports_post_process` or `supports_wrapper_mode` to `Yes`.

This can make RTK appear installed and available while actual invocation resolves to `RtkInvocationMode::Disabled`. That is safe, but it means Phase 6 may not actually compress in real use.

Corrective action:

- Add explicit probes for post-process and wrapper support.
- Document the exact RTK CLI contract being used.
- If post-process is unsupported by RTK, do not advertise it.
- If wrapper mode is supported but unsafe for exact raw retention, keep it opt-in or narrow.
- Add tests where capabilities are explicitly `Yes`, `No`, and `Unknown` for both modes.

Acceptance:

- An installed RTK binary yields deterministic capability states.
- `projection = "rtk"` either uses a verified invocation mode or falls back with a clear warning.
- There is no ambiguous “available but permanently disabled for unknown reasons” state.

### 2. Post-process mode must match RTK's real CLI contract

The current post-process path appears to spawn the RTK binary with piped stdin and no explicit subcommand/flag. This is only correct if RTK supports stdin compression in that form. If RTK only works as a command wrapper, the current post-process path will fail and fall back.

Corrective action:

- Verify RTK's supported compression modes with a real installed binary.
- If stdin post-processing requires a flag or subcommand, implement that exact invocation.
- If RTK has no post-process mode, remove or disable post-process support and document wrapper-only limitations.
- Add an optional integration test guarded by RTK availability that passes known input and verifies compressed output is non-empty and semantically associated with the input.

Acceptance:

- Post-process mode is either known-good and tested, or explicitly disabled.
- Failure mode is safe fallback, not silent empty/incorrect projection.
- Docs and config do not imply post-process support if unavailable.

### 3. Wrapper mode uses naive whitespace splitting

The current wrapper path appears to split the command string using whitespace and pass those tokens to RTK. That is not shell-equivalent and will mishandle quoting, escaping, spaces in paths, shell variables, globs, pipelines, redirects, and compound commands.

Corrective action:

- Prefer wrapper mode only when `CommandRun` has a trusted `argv` representation.
- If only a raw shell string is available, restrict wrapper mode to a narrow simple-command grammar.
- Reject wrapper mode for commands containing quotes, backslashes, shell metacharacters, pipes, redirects, command substitution, env assignments, globs, semicolons, `&&`, `||`, or newlines.
- Preserve cwd when wrapper mode is used.
- Ensure wrapper mode cannot run commands that policy would otherwise block or warn without the same policy path.

Acceptance:

- Wrapper mode never performs broader shell interpretation than intended.
- Commands with complex shell syntax fall back to safe native/generic projection.
- Tests cover quoted args, paths with spaces, pipelines, redirects, env assignments, and simple allowed commands.

### 4. Wrapper raw-handle semantics need an explicit label

Wrapper mode executes `rtk <command>` rather than the original command. Unless RTK can emit both original raw output and compressed output, raw handles attached to the wrapper path may represent RTK output rather than original command output.

Corrective action:

- Add metadata that distinguishes original raw output, RTK-wrapped output, and RTK-compressed projection.
- If original raw output is not retained in wrapper mode, mark `raw_available = false` or `raw_semantics = WrappedOutputOnly` for that projection.
- Prefer disallowing wrapper mode unless the user opts into lossy wrapped execution semantics.
- Ensure TUI/model metadata does not say “raw retained” when only RTK output is retained.

Acceptance:

- Expansion handles are truthful for all RTK modes.
- Model-facing projection says whether the raw artifact is original raw output or wrapper output.
- Tests cover post-process and wrapper raw-handle metadata separately.

### 5. RTK capability probes should not rely on shell-specific assumptions without guards

Capability probes using `sh -c` are fine on Unix-like systems but should be platform-gated or replaced with portable test binaries/commands if the repo targets non-Unix environments.

Corrective action:

- Gate `sh -c` probes under Unix.
- On non-Unix, mark relevant capabilities `Unknown` or use platform-appropriate probes.
- Ensure tests do not fail on unsupported platforms.

Acceptance:

- RTK probing is portable or explicitly platform-gated.
- CI on non-Unix platforms, if any, skips unsupported probes cleanly.

### 6. Redaction should be fuzzed for false positives and catastrophic regex behavior

The new redactor is deterministic and useful, but regex-based redaction can create two classes of bugs: false positives that destroy useful diagnostics, and pathological input that causes high CPU usage.

Corrective action:

- Add a small corpus of ordinary compiler/test/prose outputs containing words such as token, key, secret, password, cookie, and bearer in non-sensitive contexts.
- Add large-line tests to ensure redaction time is bounded enough for command output paths.
- Add property/fuzz-like tests if the repo already has a test strategy for randomized strings.
- Ensure redaction markers never include original sensitive values or derived hashes.

Acceptance:

- Redaction does not mangle ordinary diagnostics.
- Redaction handles long lines within reasonable time.
- Replacement metadata never leaks sensitive values.

### 7. Expansion and redaction boundaries need explicit model-vs-local tests

Expansion UX is now present, but the important invariant is: ephemeral `!command` output remains local unless promoted. Model-visible expansion must be redacted; local TUI detail may show raw output according to policy.

Corrective action:

- Add tests for `!command` local expansion versus `!!command` model-visible projection/expansion.
- Add tests that model-visible expansion invokes redaction.
- Add tests that local TUI detail follows local display policy.
- Ensure expansion errors do not fall back to rerunning commands.

Acceptance:

- Ephemeral shell output cannot become model context by accidental handle expansion.
- Model-visible expansion is redacted.
- Local expansion can show raw output if policy allows.

### 8. Context-budget metadata should preserve projection warnings

Phase 10 adds `ProjectionContextMetadata`, facts, and double-compression prevention. Ensure warnings from RTK fallback, redaction, partial raw artifacts, and omitted ranges survive compaction.

Corrective action:

- Add `ProjectionFact` or metadata fields for warnings and fallback reasons if not already present.
- Ensure compaction-preservation tests include RTK fallback warnings and redaction state.
- Ensure `is_already_projected` prevents destructive re-summarization but still allows compact metadata retention.

Acceptance:

- Compaction preserves command ID, raw handles, exit status, critical facts, redaction state, and backend warnings.
- Already-projected output does not get compacted into an unhelpful one-line summary.

### 9. CI coverage needs to run the new harnesses

Connector status did not show hosted CI runs for the latest head. The implementation commits mention local checks, but the new harness files need to be part of the normal validation path.

Corrective action:

- Confirm CI workflows run `cargo test --all-features` or at least include:
  - `tests/shell_projection_harness.rs`
  - `tests/shell_projection_phase10.rs`
  - shell module unit tests
- Add an optional RTK integration job or local script that runs only when RTK is installed.
- Ensure RTK-absent environments skip optional integration tests cleanly.
- Update docs with exact validation commands and optional RTK validation command.

Acceptance:

- Normal CI catches regressions in projection, redaction, expansion, and context metadata.
- Optional RTK tests are available but do not make RTK a required dependency.
- CI status is visible on PR/main pushes if the repo expects hosted checks.

## Implementation Order

1. Add RTK capability-state tests that expose the current mode-selection behavior.
2. Fix `probe_capabilities()` to set post-process/wrapper support based on real probes or explicit unsupported states.
3. Harden wrapper mode parsing and cwd/policy behavior.
4. Clarify raw-handle semantics for wrapper mode.
5. Add redaction false-positive and long-line tests.
6. Add expansion model/local boundary tests.
7. Add compaction-warning preservation tests.
8. Update docs and skills.
9. Run full validation.

## Suggested Tests

Add or update tests for:

- `rtk_capabilities_unknown_modes_disable_invocation`
- `rtk_probe_sets_post_process_yes_only_when_cli_contract_works`
- `rtk_probe_sets_wrapper_yes_only_when_wrapper_contract_works`
- `rtk_wrapper_rejects_quoted_or_shell_complex_commands_without_argv`
- `rtk_wrapper_preserves_cwd_when_used`
- `rtk_wrapper_metadata_marks_wrapped_raw_semantics`
- `rtk_post_process_metadata_marks_original_raw_available`
- `redactor_does_not_redact_compiler_token_prose`
- `redactor_handles_long_lines_without_excessive_runtime`
- `model_visible_expansion_is_redacted`
- `ephemeral_shell_handle_not_model_readable_without_promotion`
- `compaction_preserves_rtk_fallback_warning`
- `compaction_preserves_redaction_state`

## Documentation Updates

Update:

- `architecture/human_shell.md`
- `.codegg/skills/human_shell/SKILL.md`
- `AGENTS.md`
- README human-shell line if needed

The docs should state:

- RTK is optional and disabled by default.
- RTK post-process support depends on verified CLI capability.
- RTK wrapper mode is restricted and may not provide original raw output unless explicitly supported.
- Redaction is deterministic and model-visible only by default.
- Expansion handles recover retained artifacts; they do not rerun commands.
- Optional RTK tests require RTK installed; normal CI must not require it.

## Validation Commands

Run:

```bash
cargo fmt --check
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features
scripts/check-core-boundary.sh
```

If available, run optional RTK validation:

```bash
RTK_INTEGRATION=1 cargo test --all-features rtk
```

Do not claim optional RTK validation unless RTK is actually installed and the optional test path ran.

## Success Criteria

- RTK availability, capability, and invocation-mode state are deterministic and documented.
- RTK post-process mode is either verified or disabled.
- RTK wrapper mode is safe, narrow, cwd-aware, and truthful about raw-handle semantics.
- Redaction false-positive and large-input tests pass.
- Expansion respects ephemeral/model-visible boundaries.
- Compaction preserves backend warnings and redaction state.
- CI or standard validation includes the new projection harnesses.
- Docs no longer overstate RTK readiness.

## Non-Goals

- Do not expand native projectors to more ecosystems in this pass.
- Do not make RTK default.
- Do not bundle RTK.
- Do not implement model-generated summaries.
- Do not redesign the context system beyond metadata preservation fixes.

## Handoff Notes

This pass is about making the already-landed architecture trustworthy. Keep changes narrow and test-driven. If RTK's CLI contract does not support the desired mode, prefer explicit fallback over clever wrappers. The projection system is valuable even without RTK; correctness is more important than forcing external compression to appear active.
