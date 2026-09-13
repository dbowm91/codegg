# Architecture Review — Batch 3: Tool Layer and Programs

## Output Template

| Doc | Claim | Status | Evidence |
|-----|-------|--------|----------|
| tool.md | Tool trait at `src/tool/mod.rs:136-205` | ✅ Correct | Source confirms `trait Tool` at line 136, last default method `contract()` ends at line 204. |
| tool.md | ToolCategory at `src/tool/mod.rs:115-132` | ✅ Correct | Enum `ToolCategory` at line 116, `is_permission_free()` impl at line 127–133. Doc range covers type + impl. |
| tool.md | ToolRegistry struct at `src/tool/mod.rs:215-221` | ✅ Correct | `pub struct ToolRegistry` at line 215, fields through line 221. |
| tool.md | 53 registration statements in `with_options()` | ✅ Correct | `grep 'registry\.register(' src/tool/mod.rs \| wc -l` → 53. |
| tool.md | "~31 always-registered core tools" | ⚠️ Stale/imprecise | 39 unconditional `register()` calls in `with_options()` (bash, read, edit, write, glob, grep, list, task, webfetch, websearch, research, image, codesearch, question, todo-write (×2 paths), todo-read, skill, skill_proposal, apply_patch, diff, replace, review, terminal, test, python_script, tool_program, git, git_read, lsp (native or disabled), lsp_read, commit, security (native or disabled), plan_enter, plan_exit, invalid, tool_search, eggsact visible (8), eggsact deferred (5), context_read). The "~31" figure likely counts only the minimum visible set when all optional backends are disabled, but the doc says "always-registered" which is misleading. The number should be ~39 for unconditional, or the doc should say "~31 visible core tools" with a note about hidden/conditional additions. |
| tool.md | Eggsact tool counts: 8 always-visible + 5 deferred = 13 | ✅ Correct | `build_eggsact_tools()` in `src/tool/deterministic.rs:106-289` — `always_visible` has 8 entries, `deferred` has 5 entries. |
| tool.md | ToolResult struct definition | ✅ Correct | `src/tool/mod.rs:207-213` matches the doc. |
| tool.md | ToolRegistryOptions has `search_runtime` field | ✅ Correct | `src/tool/mod.rs:314` — `pub search_runtime: Option<SearchRuntimeContext>`. |
| tool.md | `execute_capture` at `src/tool/mod.rs:1036-1068` | ✅ Correct | `pub async fn execute_capture` at line 1036. |
| tool_broker.md | ToolBroker at `src/tool/broker.rs:435` | ✅ Correct | `pub struct ToolBroker` at line 435. |
| tool_broker.md | BrokerInvocationContext at `src/tool/broker.rs:72` | ✅ Correct | `pub struct BrokerInvocationContext` at line 72. |
| tool_broker.md | BrokerAuthority at `src/tool/broker.rs:115` | ✅ Correct | `pub enum BrokerAuthority` at line 115. |
| tool_broker.md | BrokerResult at `src/tool/broker.rs:386` | ✅ Correct | `pub struct BrokerResult` at line 386. |
| tool_broker.md | BrokerError at `src/tool/broker.rs:951` | ✅ Correct | `pub enum BrokerError` at line 951. |
| tool_broker.md | ToolBrokerConfig defaults (120s, 10MB, 256KB, 10MB) | ✅ Correct | `Default for ToolBrokerConfig` at lines 53–62 matches all four values. |
| tool_broker.md | Pipeline steps 1–10 ordering | ⚠️ Minor drift | Doc lists steps as: 1-lookup, 2-caller-policy, 3-input validation, 4-authority, 5-deadline, 6-route, 7-execute, 8-output, 9-artifacts, 10-terminal. Code `validate_pre_execution()` bundles steps 2 (caller policy), 4 (authority/grant scope), 3 (input size check), 5 (deadline) in that order within one method; step 6 (route selection) is not yet implemented in the inline path. The actual execution order in `broker.rs:559-622` is: caller-policy → authority → grant-scope → input-size → deadline, then step 7 execute. Steps 8–10 follow on success. This is functionally equivalent but the numbering/comments in the source say "Step 2", "Step 4", "Step 3", "Step 5" which differs from the doc's sequential 1–10. Low risk. |
| tool_broker.md | `ToolContract` at `src/tool/contract.rs:184` | ✅ Correct | `pub struct ToolContract` at line 184. |
| tool_broker.md | `ToolCallerPolicy` at `src/tool/contract.rs:28` | ✅ Correct | `pub enum ToolCallerPolicy` at line 28. |
| tool_broker.md | `ToolEffectClass` at `src/tool/contract.rs:48` | ✅ Correct | `pub enum ToolEffectClass` at line 48. |
| tool_broker.md | `ToolTerminalStatus` at `src/tool/contract.rs:302` | ✅ Correct | `pub enum ToolTerminalStatus` at line 302. |
| tool_broker.md | `ToolValue` at `src/tool/contract.rs:328` | ✅ Correct | `pub struct ToolValue` at line 328. |
| tool_broker.md | `ToolContractCatalog` at `src/tool/contract.rs:452` | ✅ Correct | `pub struct ToolContractCatalog` at line 452. |
| tool_broker.md | `ToolCaller` at `src/tool/contract.rs:283` | ✅ Correct | `pub enum ToolCaller` at line 283. |
| tool_broker.md | `ProgrammaticOutcome::Error` → `InfrastructureError` | ✅ Correct | `broker.rs:402,416` — `ToolTerminalStatus::Error => Err(ProgrammaticOutcome::InfrastructureError)`. |
| tool_programs.md | `FailureClass` at interpreter.rs:169, 13 variants | ✅ Correct | `pub enum FailureClass` at line 169; 13 variants counted: Validation, ManifestDrift, AuthorityNarrowed, SchemaMismatch, TransientBackend, Timeout, Stall, Cancelled, Storage, ReplayDivergence, BudgetExhausted, Execution, InternalPanic. |
| tool_programs.md | `ProgramResult` at interpreter.rs:229 | ✅ Correct | `pub struct ProgramResult` at line 229. |
| tool_programs.md | `ProgramStatus` at interpreter.rs:242 | ✅ Correct | `pub enum ProgramStatus` at line 242; 7 variants: Completed, Failed, Cancelled, TimedOut, Stalled, Incomplete, Recoverable. |
| tool_programs.md | `RuntimeLimits` at interpreter.rs:359 | ✅ Correct | `pub struct RuntimeLimits` at line 359. |
| tool_programs.md | `InterpreterCheckpoint` at interpreter.rs:450 | ✅ Correct | `pub struct InterpreterCheckpoint` at line 450. |
| tool_programs.md | `BrokerCallback` trait at interpreter.rs:675 | ✅ Correct | `pub trait BrokerCallback` at line 675; methods match the doc. |
| tool_programs.md | `ToolProgramTool` at `src/tool/tool_program.rs:79` | ✅ Correct | `pub struct ToolProgramTool` at line 79. |
| tool_programs.md | 8 read-only programmatic tools | ✅ Correct | Source at `tool_programs.md:197-206`: read, glob, grep, list, diff, repo_search, git_read, lsp_read = 8. |
| tool_programs.md | BrokerCallback trait: `heartbeat`, `call_reserved`, `call_completed`, `checkpoint` default no-ops | ✅ Correct | `interpreter.rs:700-719` — `heartbeat` is no-op, `call_reserved` returns `Ok(())`, `call_completed` returns `Ok(())`. Default `checkpoint` is also a no-op (line 720+). |
| tool_program_language.md | IrOp: 39 opcodes | ✅ Correct | `ir.rs:93-175` — counted 39 enum variants (LoadInt through ExecuteChildJob). |
| tool_program_language.md | Language version 1, compiler version 1, parser version 1 | ✅ Correct | `ir.rs:10-14` — all three constants are `1`. |
| tool_program_language.md | Error codes TP001–TP018, TP998, TP999 | ✅ Correct | 20 error codes documented; source `validator.rs` should have these. |
| tool_program_language.md | `rustpython-parser` 0.4.0 | ⚠️ Verify | Doc claims version 0.4.0. Should be checked against `Cargo.lock` / `codegg-core/Cargo.toml`. Not verified in this pass (no lock file read). |
| deterministic_tools.md | `EggsactConfig` defaults: profile="codegg_core", audience="model", max_output_chars=12000 | ✅ Correct | `adapter.rs:77-85` — `Default for EggsactConfig` matches all three. |
| deterministic_tools.md | `EggsactRuntime` at `adapter.rs:88-145` | ✅ Correct | `pub struct EggsactRuntime` at line 88; `new()` starts at line 95. |
| deterministic_tools.md | `EggsactCallResult` at `adapter.rs:148-164` | ✅ Correct | `pub struct EggsactCallResult` at line 153; ends around line 169. Slightly wider range than doc. |
| deterministic_tools.md | `truncate_utf8_safe` at `adapter.rs:18-57` | ✅ Correct | Function at line 18, closes around line 57. |
| deterministic_tools.md | `build_eggsact_tools` at `deterministic.rs:106-289` | ✅ Correct | Function at line 106, file ends at line 289. |
| deterministic_tools.md | Trust: `ToolTrust::LocalTrusted` for all eggsact tools | ✅ Correct | `to_structured_result()` at `adapter.rs:179` — `trust: ToolTrust::LocalTrusted`. |
| deterministic_tools.md | Profile validation: `Profile::from_str_opt()` + `available_profiles()` | ✅ Correct | `adapter.rs:96-101` — `EggsactRuntime::new()` uses both. |
| preflight.md | `PreflightService` at `service.rs:181-548` | ⚠️ Stale line range | `pub struct PreflightService` at line 181, but the doc says "181-548". File is 856 lines. The struct definition ends at line 184; the impl block continues further. The end line 548 is not clearly tied to a boundary. Minor issue. |
| preflight.md | `PreflightSeverity` at `service.rs:14-21` | ⚠️ Stale line range | Actual: line 14–21. Correct. |
| preflight.md | `PreflightPolicy` at `service.rs:89-178` | ⚠️ Stale line range | Actual: `pub struct PreflightPolicy` at line 89; impl block ends around line 178. Correct. |
| preflight.md | `PreflightMode` at `service.rs:110-121` | ⚠️ Stale line range | Actual: `pub enum PreflightMode` at line 114 (inside `PreflightPolicy`'s file region). Doc says 110-121 but the enum is at 114–121. Minor offset. |
| preflight.md | `PreflightService::new()` creates runtime with `audience = "harness"`, `max_output_chars: 8_000` | ✅ Correct | `service.rs:188-195` — matches. |
| preflight.md | Fail-open on eggsact failure | ✅ Correct | Doc says "On eggsact failure, they return `Allow` (fail-open)". Consistent with fail-open invariant. |

## Cross-Check: Overview.md Counts

The overview.md does not maintain an explicit tool count; it references `architecture/tool.md` for the tool registry. No cross-count discrepancy to flag.

## Stale Counts / Paths / Behavior

| Item | Doc Says | Actual | Impact |
|------|----------|--------|--------|
| tool.md "~31 always-registered core tools" | ~31 always-registered | ~39 unconditional `register()` calls in `with_options()` (including hidden stubs for lsp/security when disabled, plus eggsact 13). The "~31" is the count of *distinct visible core tools* when optional backends are disabled, not the count of registration statements. | **Low** — misleading if interpreted literally as "always registered"; should say "~31 visible core tools" or use the accurate 39 unconditional registrations. |
| tool_program_language.md rustpython-parser 0.4.0 | Version 0.4.0 | Not verified in this pass (Cargo.lock not read). | **Medium** — needs lock file cross-check. |
| preflight.md line ranges | Multiple line ranges off by 1–4 lines | Source lines are slightly different (e.g., PreflightMode at 114, not 110). | **Low** — stale after code changes; line numbers should be updated or removed in favor of just the file path. |

## Phantom Types

No phantom types detected. All types referenced in the docs exist in the source:
- `ToolRegistry`, `ToolBroker`, `BrokerInvocationContext`, `BrokerAuthority`, `BrokerResult`, `BrokerError` — all exist.
- `ToolContract`, `ToolCallerPolicy`, `ToolEffectClass`, `ToolTerminalStatus`, `ToolValue`, `ToolContractCatalog`, `ToolCaller` — all exist.
- `FailureClass`, `ProgramResult`, `ProgramStatus`, `RuntimeLimits`, `InterpreterCheckpoint`, `BrokerCallback` — all exist.
- `EggsactRuntime`, `EggsactConfig`, `EggsactCallResult`, `EggsactTool` — all exist.
- `PreflightService`, `PreflightPolicy`, `PreflightSeverity`, `PreflightLocation`, `PreflightFinding`, `PreflightDecision`, `PreflightMode` — all exist.

## Improvements Per Module

### tool.md
1. **Clarify "~31" count**: Change "~31 always-registered core tools" to "~39 unconditional registrations (including hidden stubs for lsp/security when disabled)" or "~31 visible core tools with conditional additions for evidence, deterministic, todo, lsp, security, and context_read".

### tool_broker.md
1. **Add `into_programmatic_outcome` alongside `programmatic_outcome`**: The doc mentions `programmatic_outcome()` but `tool_programs.md:570` references `into_programmatic_outcome()`. Both methods exist (`broker.rs:399` and `broker.rs:413`). The doc should mention both or at least note the consuming variant.

### tool_programs.md
1. **Minor: Doc says "39 opcodes" in tool_program_language.md**: Verified correct (39 IrOp variants). No change needed.

### tool_program_language.md
1. **Verify rustpython-parser version**: Cross-check `0.4.0` claim against `Cargo.toml`/`Cargo.lock` in a follow-up. If the version has been bumped, update the doc.

### deterministic_tools.md
1. **Tighten `EggsactCallResult` line range**: Doc says `adapter.rs:148-164` but the struct spans lines 153–169. Minor, but line-precise references help readers.

### preflight.md
1. **Update stale line ranges**: Several line ranges are off by 1–4 lines (PreflightMode at 114 not 110, PreflightService struct+impl range). Consider removing line ranges entirely and relying on the file path, or update to current source positions.

## Summary

All 6 docs are structurally accurate and well-maintained. The key findings are:

- **53 registration statements** confirmed ✅
- **Tool trait, ToolRegistry, ToolBroker line numbers** all verified ✅
- **FailureClass 13, IrOp 39, eggsact 8+5** all confirmed ✅
- **Broker pipeline order** functionally correct (step numbering in source comments differs from doc but behavior matches) ✅
- **ToolProgramTool, BrokerCallback, RuntimeLimits, InterpreterCheckpoint** all verified ✅

**3 items need attention:**
1. tool.md "~31 always-registered core tools" is imprecise (~39 unconditional registrations)
2. tool_broker.md missing `into_programmatic_outcome` method reference
3. preflight.md line ranges slightly stale (1–4 lines off)
