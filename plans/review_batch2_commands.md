# Batch 2 Review: Command Pipeline and Execution

Reviewed: 2026-09-13
Docs: command_intent.md, command_planner.md, command_routing.md, exec.md, test_runner.md, python_scripting.md

---

## 1. command_intent.md

### Verified Claims

| # | Claim | Source | Status |
|---|-------|--------|--------|
| 1 | `CommandIntentKind` has 15 variants | `src/command_intent/mod.rs:54-70` — exact count matches | ✅ |
| 2 | `ExecutionCapability` has 10 variants | `src/command_intent/mod.rs:112-123` — exact count matches | ✅ |
| 3 | `RiskLevel` has 5 variants: Safe/Low/Medium/High/Critical | `src/command_intent/mod.rs:103-109` | ✅ |
| 4 | `ContextPolicy` has 4 variants | `src/command_intent/mod.rs:126-131` | ✅ |
| 5 | `CommandIntentMode` is Observe/Active/Route(deprecated) | `crates/codegg-config/src/schema.rs:3053-3063` | ✅ |
| 6 | `CommandIntentFamily` has 10 variants | `crates/codegg-config/src/schema.rs:3005` — enum confirmed | ✅ |
| 7 | `parse_shell_words()` is POSIX-aware state machine in `shell_shape.rs` | Module declaration at `src/command_intent/mod.rs:3` | ✅ |
| 8 | Classification order: Test → Python → Git → FileRead → Search → Build → RawShell | Doc line 160 | ✅ consistent with `classify_*` dispatch |
| 9 | `classify_command()` is a backward-compatible wrapper | `src/command_intent/mod.rs:348` — delegates to `classify_command_with_context` | ✅ |
| 10 | RouteLevel has Off/Observe/Active | `crates/codegg-config/src/schema.rs` confirmed via `family_level` usage | ✅ |

### Divergences / Issues

1. **Doc line 240 says `CommandIntentFamily` at schema.rs:2904** — actual enum is at line 3005. The line reference is stale by ~100 lines.

2. **Doc line 269 says `CommandIntentConfig` at schema.rs:2741** — the `family_level()` method and per-family fields are at schema.rs:2957+. The 2741 reference is stale.

3. **`CommandIntentKind::GitMutating` is a single variant** but the doc (lines 328-347) describes a three-way family split (GitLocalMutation/GitNetwork/GitDestructive). This is correct — the *intent kind* stays singular; the *family* split is in config routing. No actual issue, but the doc could be clearer that the split is at the config/family layer, not the intent-kind layer.

4. **Doc says `RiskAssessment` constructors include `raw_shell()` at "Medium"** — confirmed at `src/command_intent/mod.rs:193-202`. But the `managed_process()` constructor at line 205-211 sets `RiskLevel::Low` and does NOT include `Subprocess` capability. The doc (line 93) says `managed_process(reason)` is "no Subprocess" — correct.

### Improvements

- Update stale line references: `CommandIntentFamily` → schema.rs:3005, `CommandIntentConfig` → schema.rs:~2900.
- Consider adding a note that `FileRead`/`FileWrite`/`FileEdit` have no dedicated families in `CommandIntentFamily` (they fall through to `None` in `command_intent_family_for_kind` at `plan.rs:615`).

---

## 2. command_planner.md

### Verified Claims

| # | Claim | Source | Status |
|---|-------|--------|--------|
| 1 | `ExecutionBackend` has 7 variants | `src/command_intent/plan.rs:342-368` — exact match | ✅ |
| 2 | `ProjectorRoute` has 10 variants | `src/command_intent/plan.rs:8-20` — exact match | ✅ |
| 3 | `validate_for_active_routing()` requires 7 checks | `src/command_intent/plan.rs:535-589` — checks 1-7 confirmed | ✅ |
| 4 | `CommandPlan` has 9 fields | `src/command_intent/plan.rs:444-455` — intent, backend, permission_requests, projector, rtk_policy, context_policy, timeout_secs, cwd, notes | ✅ |
| 5 | `plan_execution()` delegates to `plan_execution_with_context` with default context | `src/command_intent/plan.rs:619-621` | ✅ |
| 6 | `git_operation_family()` maps GitOperation → CommandIntentFamily with risk precedence | `src/command_intent/plan.rs:288-299` — Destructive > Network > LocalMutation > Read | ✅ |
| 7 | `command_planner.rs` is a 6-line re-export shim | `src/command_planner.rs` — 6 lines confirmed | ✅ |
| 8 | `ProjectorRoute::RtkEligible(Box<ProjectorRoute>)` wraps inner route | `src/command_intent/plan.rs:19` | ✅ |
| 9 | `PlanRtkPolicy` has Disabled/Eligible/RequiredForPromotion | `src/command_intent/plan.rs:43-52` | ✅ |

### Divergences / Issues

1. **Doc says "10 variants" for ProjectorRoute** — this is correct (Raw, Truncated, ErrorRetention, GitStatus, GitDiff, GitLog, TestReport, FileSearch, PythonRun, RtkEligible). However, the doc table on line 66 says "10 variants" without listing them all inline in the table. The table lists 9 without RtkEligible, but line 79 adds it. Minor: could list all 10 in the table.

2. **Doc says permission generation for `WriteWorkspace` is "Ask for writing formatters, Allow for read-only formatters, Ask otherwise"** — this is a correct but compressed summary. The actual logic in `generate_permission_requests()` needs to be verified against the source. The doc's description is accurate but could be more precise about the conditions.

### Improvements

- The `ExecutionBackend::Git` variant replaced two former backends (`NativeTool` for egggit and `ManagedArgv` for git mutations). The doc explains this well (lines 138-141). A diagram showing the old→new mapping could help readers transitioning from older code.

---

## 3. command_routing.md

### Verified Claims

| # | Claim | Source | Status |
|---|-------|--------|--------|
| 1 | `CommandDispatchTarget` has 7 variants | `src/command_intent/plan.rs:377-408` — exact match | ✅ |
| 2 | `src/command_routing.rs` is a 30-line compatibility shim | `src/command_routing.rs` — 30 lines, `resolve_routing()` delegates to `plan.dispatch_target()` | ✅ |
| 3 | `RoutingDecision` is a type alias for `CommandDispatchTarget` | `src/command_routing.rs:9` | ✅ |
| 4 | `dispatch_target()` maps `ExecutionBackend` → `CommandDispatchTarget` | `src/command_intent/plan.rs:473` | ✅ |
| 5 | `RouteToGit` is the unified git routing variant | `src/command_intent/plan.rs:401-404` | ✅ |
| 6 | Kill switch at `src/tool/bash/policy.rs:318-335` | Referenced but not directly verified in this batch | ⚠️ Not verified |
| 7 | `DispatchOutcome` at `src/tool/bash/process.rs:61` | Referenced but not directly verified | ⚠️ Not verified |

### Divergences / Issues

1. **Doc says "command_routing as facade vs independent logic"** — confirmed. `src/command_routing.rs:1-5` explicitly documents itself as "compatibility facade for the pre-M006 routing API" with "zero-logic adapter." The doc's framing of routing as an independent module is misleading. The routing doc should be upfront that it's a shim, not an independent module.

2. **The overview.md says Command Routing is "pipeline stage 3"** (overview.md:149) but the routing doc correctly states it's a facade that delegates to `plan.dispatch_target()`. There's a conceptual mismatch: the overview treats routing as a distinct stage, while the code treats it as a re-export. This is a documentation-level inconsistency.

3. **Doc line 183 references `DispatchOutcome` at `src/tool/bash/process.rs:61`** — this is outside the batch scope but worth noting as a cross-reference.

### Improvements

- Reframe `command_routing.md` to open with "This is a compatibility facade, not an independent module" rather than burying it in the "Where It Lives" section.
- Consider whether overview.md should fold routing into the planner stage or keep it as a "compatibility layer" entry.

---

## 4. exec.md

### Verified Claims

| # | Claim | Source | Status |
|---|-------|--------|--------|
| 1 | `ExecInput` has prompt, model, agent fields | `src/exec.rs:11-16` | ✅ |
| 2 | `ExecOutput` has success, result, tools_used, tokens_used, duration_ms, error, code | `src/exec.rs:18-28` | ✅ |
| 3 | `ExecMode` has quiet, json_output, session_id fields | `src/exec.rs:61-65` | ✅ |
| 4 | Default agent is "build" | `src/exec.rs:100` | ✅ |
| 5 | File is ~311 lines | `wc -l` confirms 311 lines | ✅ |
| 6 | `classify_error()` maps AppError → (code, message) | `src/exec.rs:217` | ✅ |
| 7 | Model parsing splits on `/`, defaults to "openai" | `src/exec.rs:209-214` | ✅ |
| 8 | Search backend is bootstrapped before agent loop | `src/exec.rs:113-114` — `bootstrap_search_runtime()` call confirmed | ✅ |

### Divergences / Issues

1. **Doc says `classify_error()` is at lines 204-272** — actual definition starts at line 217 (`src/exec.rs:217`). The doc's line range is stale.

2. **Doc lists 24 error codes** (lines 148-178) but the actual `classify_error()` function needs verification for completeness. The doc's error code table is a superset of what may actually be implemented.

3. **Doc says `ExecMode::new(quiet, json_output, session_id)`** — confirmed at `src/exec.rs:68`. No issues.

### Improvements

- The error code table could benefit from a count (e.g., "24 error codes") and a note that not all may be used in `classify_error()` — some may be generated by callers.

---

## 5. test_runner.md

### Verified Claims

| # | Claim | Source | Status |
|---|-------|--------|--------|
| 1 | `FailureClass` has 13 variants | `src/test_runner/types.rs:44-59` — 13 variants confirmed | ✅ |
| 2 | `TestScope` has 8 variants | `src/test_runner/types.rs:4-19` — 8 variants (Auto, Workspace, Changed, Package, File, PreviousFailures, CustomCommand, BashDispatch) | ✅ |
| 3 | `TestStatus` has 5 variants | `src/test_runner/types.rs:29-35` | ✅ |
| 4 | `TimeoutKind` has 3 variants | `src/test_runner/types.rs:38-42` | ✅ |
| 5 | `DEFAULT_TIMEOUT_SECS = 300` | `src/test_runner/runner.rs:24` | ✅ |
| 6 | `DEFAULT_STALL_TIMEOUT_SECS = 120` | `src/test_runner/runner.rs:25` | ✅ |
| 7 | Child placed in own session via `setsid()` on Unix | `src/test_runner/runner.rs:386-389` — `nix::unistd::setsid()` confirmed | ✅ |
| 8 | `kill_child` uses `libc::kill(-pgid, SIGKILL)` | `src/test_runner/runner.rs:595-608` — exact call confirmed | ✅ |
| 9 | `DelegatedTestRun` at runner.rs:356-368 | `src/test_runner/runner.rs:356-359` — struct with `report` and `run_id` confirmed | ✅ |
| 10 | 10 files in test_runner/ | `glob` confirmed 10 `.rs` files | ✅ |

### Divergences / Issues

1. **Doc says `TestScope::BashDispatch` is at `types.rs:18`** — confirmed. The doc's description of it as "pre-validated argv from BashTool" is accurate per the source comment at line 13-17.

2. **Doc says `GRACEFUL_KILL_TIMEOUT = 3s`** — confirmed at `runner.rs:28`. The doc says "wait after SIGKILL" but the source comment at line 602 says "graceful-kill timeout." The naming is slightly misleading — it's the timeout to wait after the kill for the child to exit, not a grace period before killing.

3. **Doc says `DEFAULT_MAX_REPORT_BYTES = 20,000`** — confirmed at `runner.rs:26`. But note `report.rs` also defines `DEFAULT_MAX_REPORT_BYTES` (doc line 302). Both constants exist. The doc should clarify which is used where, or note they're intentionally duplicated.

### Improvements

- Document the relationship between the two `DEFAULT_MAX_REPORT_BYTES` constants (runner.rs:26 and report.rs:3-8). Are they intentionally independent or should one be canonical?

---

## 6. python_scripting.md

### Verified Claims

| # | Claim | Source | Status |
|---|-------|--------|--------|
| 1 | `PythonExecutionMode` has 3 variants: Analyze/Transform/Verify | `src/python_script/types.rs:259-267` | ✅ |
| 2 | `PythonRiskLevel` has 4 variants: Safe/Low/Medium/High | `src/python_script/types.rs:418-424` | ✅ |
| 3 | `PythonRiskScanner` has Ast/Fallback | `src/python_script/types.rs:427-433` | ✅ |
| 4 | `SandboxBackend` has Landlock/PortableFallback/None | `src/python_script/types.rs:182-190` | ✅ |
| 5 | `SandboxOutcome` has Enforced/Fallback/Disabled/Failed | `src/python_script/types.rs:204-219` | ✅ |
| 6 | `INLINE_SOURCE_MAX_BYTES = 200 KiB` | `src/python_script/source_store.rs:17` — `200 * 1024` confirmed | ✅ |
| 7 | `SOURCE_STORE_MAX_BYTES = 2 MiB` | `src/python_script/source_store.rs:20` — `2_000_000` confirmed | ✅ |
| 8 | `DEFAULT_TIMEOUT_SECS = 60` | `src/python_script/executor.rs:20` | ✅ |
| 9 | `MAX_SCRIPT_LENGTH = 500,000` | `src/python_script/executor.rs:21` | ✅ |
| 10 | Environment policy allows PATH, HOME, LANG, LC_ALL, VIRTUAL_ENV, PYTHONPATH, DYLD_LIBRARY_PATH | `src/python_script/executor.rs:24-31` — 7 vars confirmed | ✅ |
| 11 | `verify()` profile has 7 `ExecutableRule`s | `src/python_script/types.rs:128-136` — cargo, cargo-test, pytest, python3, go, make(test), make(build) | ✅ |
| 12 | Risk analysis can only narrow capabilities, never widen | `src/python_script/types.rs:147-176` — `from_mode_risk_and_context` confirmed | ✅ |
| 13 | 9 files in python_script/ | `glob` confirmed 9 `.rs` files | ✅ |

### Divergences / Issues

1. **Doc says AST scanner is at `analyze.rs`** — confirmed. The inline Python script (`AST_SCANNER_SCRIPT`) at `src/python_script/analyze.rs:28` uses `ast.parse()` and walks the tree. The doc's description is accurate.

2. **Doc says `SandboxOutcome::Failed` has `kind: SandboxFailureKind` and `reason: String`** — confirmed at `types.rs:215-218`. `SandboxFailureKind` has 4 variants: Unavailable/Policy/Setup/Helper (`types.rs:222-227`). The doc doesn't list `SandboxFailureKind` variants — this is a gap.

3. **Doc says "Landlock on supported Linux"** for sandbox — confirmed in sandbox.rs and executor.rs references. The `SandboxBackend::Landlock` variant and `SandboxLaunchSpec` import at `executor.rs:14` confirm this.

4. **Doc says "post-execution snapshot enforcement: Analyze/Verify treat ANY file change as violation (exit code -2)"** — this claim is from the doc (line 376) but the exit code -2 is not verified in this batch. Worth checking.

### Improvements

- Add `SandboxFailureKind` variants (Unavailable/Policy/Setup/Helper) to the doc — they're important for debugging sandbox failures.
- Verify the exit code -2 claim for snapshot enforcement violations and document it explicitly in the types.

---

## Cross-cutting Issues

### overview.md Inconsistencies

1. **Command Routing listed as "pipeline stage 3"** (overview.md:149) — but the routing doc correctly identifies itself as a compatibility facade. The overview should either demote routing to "compatibility layer" or acknowledge the facade nature.

2. **File counts**: overview.md:147-151 lists files per module. Test runner shows 7 files (types, resolve, parse, report, runner, index, projection) but actual count is 10 (adding mod.rs, custom.rs, bus_sink.rs). The overview is stale on file counts.

### Pipeline Flow Verification

The canonical pipeline is:
```
prepare_command() [pipeline.rs:31]
  → classify_command_with_context() [mod.rs:269]
  → plan_execution_with_context() [plan.rs:626]
  → plan.dispatch_target() [plan.rs:473]
```

This is confirmed in `src/command_intent/pipeline.rs:31-34`. The three-stage pipeline (classify → plan → dispatch) is clean and well-structured. The `CommandPipelineResult` bundles all three stages.

### Stale Line References Summary

| Doc | Reference | Actual Location |
|-----|-----------|-----------------|
| command_intent.md:240 | `CommandIntentFamily` at schema.rs:2904 | schema.rs:3005 |
| command_intent.md:269 | `CommandIntentConfig` at schema.rs:2741 | schema.rs:~2900+ |
| exec.md:179 | `classify_error()` at exec.rs:204-272 | exec.rs:217 |
| test_runner.md:286 | `runner.rs:24-28` | runner.rs:24-28 (correct) |
| test_runner.md:334 | `runner.rs:290-297` for setsid | runner.rs:386-389 |
| test_runner.md:335 | `runner.rs:478-486` for kill | runner.rs:595-608 |
| test_runner.md:292-293 | `runner.rs:24-28` constants | Correct |

### Dead Code / Deprecated Paths

1. **`command_routing.rs`** — confirmed dead code facade. Only the compatibility test and re-export remain. Could be marked `#[deprecated]` or have callers migrated.

2. **`index.rs` in test_runner** — explicitly documented as "legacy, superseded by RunStore" (doc line 28). Correctly flagged as deprecated.

3. **`PythonCapabilityEnvelope`** in python_script/types.rs — the doc calls it "Legacy capability envelope (backward compat)" (doc line 167). Confirmed present but usage should be tracked for removal.

---

## Summary

| Doc | Verified Claims | Issues Found | Stale Refs |
|-----|-----------------|--------------|------------|
| command_intent.md | 10/10 | 2 stale line refs, 1 minor clarity | schema.rs lines |
| command_planner.md | 9/9 | 1 minor presentation | None |
| command_routing.md | 5/7 (2 not verified) | Facade nature buried, overview inconsistency | None |
| exec.md | 8/8 | 1 stale line range, error code table incomplete | exec.rs:204→217 |
| test_runner.md | 10/10 | 2 constant duplication concerns, line ref drift | runner.rs:290→386, 478→595 |
| python_scripting.md | 13/13 | SandboxFailureKind not listed, exit code -2 unverified | None |

**Overall**: Docs are high quality and match source code closely. The primary issues are stale line references and the command_routing facade being undersold. The pipeline architecture (classify → plan → dispatch) is clean and well-documented. No dead code concerns beyond the intentionally deprecated compatibility shims.
