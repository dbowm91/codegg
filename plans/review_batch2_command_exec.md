# Review: batch2 command-exec
**Reviewed**: 2026-09-11
**Files**: command.md, command_intent.md, command_planner.md, command_routing.md, exec.md, test_runner.md, python_scripting.md

## Summary

All 7 docs are structurally sound and their behavioral descriptions (enum variants, classification order, pipeline stages, routing logic) are accurate. The primary issues are stale/incorrect line-number references, a few wrong file locations for functions that were moved during refactoring, and minor numerical discrepancies. No wrong enum variant counts or incorrect behavior descriptions were found.

## Documentation Issues

| # | File | Line | Issue | Suggested fix |
|---|------|------|-------|---------------|
| 1 | command_routing.md | 129 | `check_kill_switches` cited at `src/tool/bash.rs:494-511` — function is actually in `src/tool/bash/policy.rs:318-335`. The file was refactored into a sub-module. | Update to `src/tool/bash/policy.rs:318-335` |
| 2 | command_routing.md | 183 | `DispatchOutcome` cited at `src/tool/bash.rs:36-41` — struct is in `src/tool/bash/process.rs:61`. | Update to `src/tool/bash/process.rs:61` |
| 3 | command_planner.md | 161 | `validate_for_active_routing()` cited at `plan.rs:431` — actual location is `plan.rs:535`. | Update to `plan.rs:535` |
| 4 | command_intent.md | 225 | `classify_command()` cited as called at `src/tool/bash.rs:1671` — bash.rs is only 1081 lines. The call lives in `src/tool/bash/policy.rs`. | Update to `src/tool/bash/policy.rs` (remove line number or correct) |
| 5 | test_runner.md | 194 | `DelegatedTestRun` cited at `runner.rs:260-272` — actual location is `runner.rs:356-368`. | Update to `runner.rs:356-368` |
| 6 | command_planner.md | 246 | `persist_python_run` cited at `src/python_script/tool.rs:220` — actual location is `tool.rs:232`. | Update to `tool.rs:232` |
| 7 | python_scripting.md | 351 | Diff cap cited at `executor.rs:669` — `MAX_DIFF_CONTENT` constant is at `executor.rs:688`. | Update to `executor.rs:688` |
| 8 | python_scripting.md | 352 | 2 MiB per-file capture cited at `executor.rs:598` — `MAX_FILE_BYTES` constant is at `executor.rs:617`. | Update to `executor.rs:617` |
| 9 | exec.md | 12 | `src/exec.rs` stated as `~298 lines` — file is 311 lines. | Update to `~311 lines` |
| 10 | command_planner.md | 12 | `src/command_planner.rs` stated as `5 lines` — file is 6 lines. | Update to `6 lines` |
| 11 | test_runner.md | 234 | `TestParseState` cited at `parse.rs:20-30` — struct definition starts at `parse.rs:20` (correct start, but check exact line if file changed). | Verify exact lines; minor drift |
| 12 | test_runner.md | 285 | Runner constants cited at `runner.rs:24-28` — correct (24-28). ✓ | No change needed |
| 13 | test_runner.md | 115 | `TestRunRequest` cited at `types.rs:121-128` — actual location is `types.rs:121-128`. ✓ | No change needed |
| 14 | test_runner.md | 76 | `FailureClass` cited at `types.rs:44-59` — actual location is `types.rs:44-59`. ✓ | No change needed |
| 15 | test_runner.md | 128 | `ResolvedTestCommand` cited at `types.rs:131-136` — actual location is `types.rs:131-136`. ✓ | No change needed |

## Code Issues Found

No genuine code issues found. All verified behavioral claims match the source:
- 139 built-in commands confirmed via test at `src/tui/command.rs:849-851`
- 3 Python modes (`Analyze`, `Transform`, `Verify`) confirmed at `python_script/types.rs:260-267`
- 15 `CommandIntentKind` variants confirmed at `command_intent/mod.rs:54-70`
- 10 `ExecutionCapability` variants confirmed at `command_intent/mod.rs:112-123`
- 10 `ProjectorRoute` variants confirmed at `command_intent/plan.rs:9-20`
- 7 validation checks in `validate_for_active_routing()` confirmed at `plan.rs:535-593`
- 12-entry custom command allowlist confirmed at `test_runner/custom.rs:22-71`
- 13 `FailureClass` variants confirmed at `test_runner/types.rs:44-59`
- `is_safe_for_model_context()` logic matches doc at `mod.rs:252-258`
- `run_kind_for_outcome()` unconditional RawShell mapping confirmed at `command_outcome.rs:162`
- `DelegatedPythonRun` at `tool.rs:16-19` matches doc
- `INLINE_SOURCE_MAX_BYTES = 200 KiB` matches doc's `204,800`
- `SOURCE_STORE_MAX_BYTES = 2,000,000` matches doc's `2,000,000`

## Improvement Opportunities

1. **command_routing.md**: The doc describes `check_kill_switches` code inline but the file has been refactored into `src/tool/bash/policy.rs`. Consider updating the code snippet to reflect the current module path, or adding a note that the implementation lives in the `policy` submodule.

2. **command_intent.md**: The doc references `CommandIntentFamily` and `CommandIntentConfig` with specific line numbers in `crates/codegg-config/src/schema.rs` (lines 2741, 2904). These config schema line numbers are fragile — consider removing exact line numbers for external crate references and instead noting the type name only.

3. **test_runner.md**: The `DelegatedTestRun` line reference is off by ~96 lines (260 vs 356). This suggests the file has grown significantly since the doc was written. Consider adding a "last verified" timestamp or running a doc-freshness check in CI.

4. **exec.md**: The error code table (lines 146-178) lists 27 error codes. Consider cross-referencing with `AppError` variants in `error.rs` to ensure no new variants have been added without a corresponding doc update.

5. **python_scripting.md**: The doc mentions `python_environment_policy()` inherited env vars at `executor.rs:24-33` but the actual function is at `executor.rs:23-32`. Minor drift; consider removing exact line references for inline code blocks.

## Stale Content to Prune

1. **command_intent.md line 204-212**: The "Polish-pass provenance parity" section references `tests/git_execution_origin_matrix.rs` with "19 tests, rows 1-10". The row numbering and test count may drift. Consider replacing specific row references with a link to the test file.

2. **command_routing.md lines 155-174**: The "Polish-pass provenance parity" section duplicates content from command_intent.md. Consider deduplicating by keeping the matrix description in one doc and cross-referencing from the other.

3. **command_planner.md line 12**: The "5 lines" claim for `src/command_planner.rs` is wrong (6 lines) and will drift further as re-exports change. Consider removing the line count and just noting it's a re-export shim.
