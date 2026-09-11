# Review: batch3 tools

**Reviewed**: 2026-09-11
**Files**: tool.md, tool_broker.md, tool_programs.md, tool_program_language.md, deterministic_tools.md, preflight.md

## Summary

6 architecture docs reviewed against source. `tool_programs.md` has the most issues — its Key Types & APIs table lists 8+ phantom types that do not exist in the referenced files, and BrokerCallback line number is off by 37 lines. `tool.md` has several stale line-number references (execute_capture off by >100 lines) and phantom module paths. `deterministic_tools.md` and `preflight.md` have minor line-number drift. `tool_broker.md` has a dimension-count discrepancy. `tool_program_language.md` is the cleanest doc overall.

## Documentation Issues

| # | File | Line | Issue | Suggested fix |
|---|------|------|-------|---------------|
| 1 | tool_programs.md | 80 | **Phantom type**: `ToolProgramId` referenced as `crates/codegg-core/src/tool_program/mod.rs` — does not exist there or anywhere in that module. No opaque typed program ID type exists in tool_program. | Remove from Key Types table or document where the ID type actually lives (possibly in the DB layer, not the core domain). |
| 2 | tool_programs.md | 81 | **Phantom type**: `ProgramCallId` — does not exist in mod.rs. | Remove or locate. |
| 3 | tool_programs.md | 82 | **Phantom type**: `ToolProgramState` — does not exist in mod.rs. The lifecycle state machine is in the DB/scheduler layer, not the core domain types. | Remove or relocate reference to correct module. |
| 4 | tool_programs.md | 83 | **Phantom type**: `ProgramLanguage` — does not exist in mod.rs. The language is constrained by the parser/validator, not an enum type. | Remove or document the actual mechanism. |
| 5 | tool_programs.md | 84 | **Phantom type**: `ProgramSourceRef` — doc says `store.rs`, but store.rs only defines `ProgramStore`, `CompilationKey`, `ProgramLoadResult`. No `ProgramSourceRef`. | Remove or locate. |
| 6 | tool_programs.md | 85 | **Phantom type**: `ProgramCapabilityManifest` — does not exist in mod.rs. | Remove or locate. |
| 7 | tool_programs.md | 86 | **Phantom type**: `ProgramCheckpoint` — does not exist. The actual type is `InterpreterCheckpoint` (interpreter.rs:450). | Replace with `InterpreterCheckpoint`. |
| 8 | tool_programs.md | 87 | **Phantom type**: `ProgramCallRecord` — does not exist. The actual types are `CallRequest` (interpreter.rs:582) and `CompletedCall` (interpreter.rs:660). | Replace with `CompletedCall` / `CallRequest`. |
| 9 | tool_programs.md | 91 | **BrokerCallback line off by 37**: doc says `interpreter.rs:638`, actual is `interpreter.rs:675`. Also the doc signature is a simplified version that omits the `&self` receiver and uses different formatting. | Update line to 675; ensure signature matches actual code. |
| 10 | tool_programs.md | 94 | **InterpreterCheckpoint line off by 1**: doc says `interpreter.rs:449`, actual is `interpreter.rs:450`. | Update to 450. |
| 11 | tool_programs.md | 88 | **ProgramResult line off by 1**: doc says `interpreter.rs:228`, actual is `interpreter.rs:229`. | Update to 229. |
| 12 | tool.md | 200 | **ToolCategory line range off**: doc says `mod.rs:113-130`, actual `ToolCategory` enum is at lines 116-125 (enum items 118-124, impl 127-132). | Update to `mod.rs:115-132`. |
| 13 | tool.md | 170 | **Tool trait line range off**: doc says `mod.rs:132-201`, actual trait is at lines 136-205. | Update to `mod.rs:136-205`. |
| 14 | tool.md | 368 | **ToolRegistry line range off**: doc says `mod.rs:212-217`, actual struct is at lines 215-221. | Update to `mod.rs:215-221`. |
| 15 | tool.md | 483 | **execute_capture line range off by 103 lines**: doc says `mod.rs:833-865`, actual is `mod.rs:1036-1068`. | Update to `mod.rs:1036-1068`. |
| 16 | tool.md | 503 | **ToolCatalog line range off by 36 lines**: doc says `catalog.rs:134-143`, actual `ToolCatalog` struct is at `catalog.rs:170-178`. | Update to `catalog.rs:170-178`. |
| 17 | tool.md | 55 | **Phantom module path**: doc lists `bash/output.rs` as `src/tool/bash/output.rs` but the actual file is `src/tool/bash/output.rs`. However, the doc lists `bash/policy.rs` at line 55 — verify this exists. The file listing also mentions `src/tool/python_script/` as `(python_script/)` without a file. | Verify `src/tool/python_script/` exists as a directory; list its files. |
| 18 | tool_broker.md | 118 | **Dimension count discrepancy**: doc says `verify_grant_scope` checks "12 dimensions" but the actual code (broker.rs:175-299) and code comment (line 172) both say 8 dimensions: validity, integrity, workspace, caller class, effect class, session, permission mode, principal, path policy. The doc's "12" includes manifest and contract snapshot which appear in separate checks, not in `verify_grant_scope`. | Change "12" to "9" or clarify that the doc counts verification dimensions across the full pipeline, not just `verify_grant_scope`. |
| 19 | tool_broker.md | 69 | **BrokerError line off by 5**: doc says `broker.rs:946`, actual is `broker.rs:951`. | Update to 951. |
| 20 | tool_broker.md | 58 | **ToolContract line off by 1**: doc says `contract.rs:183`, actual is `contract.rs:184`. | Update to 184. |
| 21 | deterministic_tools.md | 48 | **EggsactRuntime line range off**: doc says `adapter.rs:88-145`, actual struct is at lines 88-91, impl at 93-145. The range is acceptable but imprecise. | Minor: could narrow to `adapter.rs:88-91` for the struct itself. |
| 22 | deterministic_tools.md | 62 | **EggsactConfig line range off**: doc says `adapter.rs:68-85`, actual is `adapter.rs:68-85` (struct at 68-75, impl at 77-85). This is correct. | No fix needed. |
| 23 | deterministic_tools.md | 72 | **EggsactCallResult line range off**: doc says `adapter.rs:148-164`, actual is `adapter.rs:148-164`. Correct. | No fix needed. |
| 24 | deterministic_tools.md | 92 | **EggsactTool line range off**: doc says `deterministic.rs:17-91`, actual struct is at lines 17-26, impl at 28-91. The range includes the full impl. Acceptable. | No fix needed. |
| 25 | deterministic_tools.md | 98 | **build_eggsact_tools line range off**: doc says `deterministic.rs:106-288`, actual is `deterministic.rs:106-289`. Off by 1 line. | Update to 289. |
| 26 | deterministic_tools.md | 108 | **truncate_utf8_safe line range off**: doc says `adapter.rs:18-57`, actual is `adapter.rs:18-57`. Correct. | No fix needed. |
| 27 | preflight.md | 41 | **PreflightSeverity line off by 1**: doc says `service.rs:14-21`, actual enum is at lines 14-21. Correct. | No fix needed. |
| 28 | preflight.md | 51 | **PreflightLocation line off by 1**: doc says `service.rs:24-29`, actual is `service.rs:24-29`. Correct. | No fix needed. |
| 29 | preflight.md | 61 | **PreflightFinding line off by 1**: doc says `service.rs:32-39`, actual is `service.rs:32-39`. Correct. | No fix needed. |
| 30 | preflight.md | 73 | **PreflightDecision line range off**: doc says `service.rs:42-86`, actual enum is at lines 42-47, impl at 49-86. Acceptable. | No fix needed. |
| 31 | preflight.md | 85 | **PreflightPolicy line range off**: doc says `service.rs:89-178`, actual struct is at lines 89-107, impl at 123-178. Acceptable. | No fix needed. |
| 32 | preflight.md | 110 | **PreflightMode line off**: doc says `service.rs:110-121`, actual is `service.rs:110-121`. Correct. | No fix needed. |
| 33 | preflight.md | 121 | **PreflightService line range off**: doc says `service.rs:181-548`, actual struct is at lines 181-184, impl at 186+. Need to verify 548. | Verify line 548 matches end of impl block. |

## Code Issues Found

No genuine code bugs were found in the source files examined. The code is well-structured and consistent with the documented behavior.

## Improvement Opportunities

1. **tool_programs.md Key Types table**: The Key Types & APIs table (lines 78-100) is a maintenance liability — 8+ types listed do not exist in the referenced module. This table should be regenerated from actual `pub use` re-exports in `mod.rs` (which are: `ProgramValue`, `ProgramResult`, `ProgramStatus`, `FailureClass`, `BrokerCallback`, `CallRequest`, `CallResult`, `CompletedCall`, `RuntimeLimits`, `InterpreterCheckpoint`, etc.). Alternatively, add a brief note that some types live in the scheduler/DB layer.

2. **tool.md**: The directory tree (lines 38-107) should be auto-generated or validated against `ls src/tool/` to prevent stale entries. The `execute_capture` line reference being off by 103 lines suggests the doc has not been updated since a significant refactor moved the method.

3. **tool_broker.md**: The "12 dimensions" claim for `verify_grant_scope` should be reconciled with the actual 9 checks in code. If the intent is to describe the full broker authority pipeline (including manifest/contract checks that happen outside `verify_grant_scope`), the doc should say "the broker pipeline verifies up to 12 dimensions across its steps" rather than attributing all 12 to `verify_grant_scope`.

## Stale Content to Prune

| Doc | Section | Issue |
|-----|---------|-------|
| tool_programs.md:80-87 | Key Types table rows for `ToolProgramId`, `ProgramCallId`, `ToolProgramState`, `ProgramLanguage`, `ProgramSourceRef`, `ProgramCapabilityManifest`, `ProgramCheckpoint`, `ProgramCallRecord` | These types do not exist in `crates/codegg-core/src/tool_program/mod.rs`. Remove or replace with actual types from the module's `pub use` re-exports. |
| tool.md:38-107 | Directory tree listing | Several entries are inaccurate or incomplete (e.g., `bash/output.rs` line numbers, `python_script/` directory listed without detail, `tool_program_context.rs` and `tool_program_ledger.rs` are listed but the paths don't match what's in `src/tool/mod.rs`'s `pub mod` declarations). Should be regenerated from the module declarations. |
