# Consolidated Architecture Documentation Review

**Source**: review_batch1 through review_batch10 (10 batch reviews, 2026-09-11)
**Reviewed docs**: 76 deep-dive architecture docs + overview.md

## Summary Counts

| Severity | Count |
|----------|-------|
| **HIGH** | 38 |
| **MEDIUM** | 52 |
| **LOW** | 74 |

---

## A) Count / Behavior Corrections

These are wrong numbers asserted in docs — wrong variant counts, wrong field counts, wrong struct descriptions, phantom types, or wrong behavior claims.

### HIGH

| # | Doc | Claim | Correct Value | Source |
|---|-----|-------|---------------|--------|
| A1 | agent.md:254 | AgentLoop "32 fields" | 38 fields | `src/agent/loop.rs:87–128` |
| A2 | cache-aware-context.md:318 | ContextPolicyDecision "6 fields" | 9 fields | `policy.rs:26` |
| A3 | git.md:39 | GitPayload "12 variants" | 13 (includes `None`) | `crates/codegg-git/src/operation.rs` |
| A4 | session.md:53 | "52+ CREATE TABLE statements" | 71 | `session/schema.rs` (matches overview) |
| A5 | session.md:109 | "22 session columns" (SESSION_COLUMNS) | 27 | `session/state.rs` |
| A6 | bus.md:18,43 | AppEvent "45 variants" | 53 | `bus/events.rs:61` (matches overview) |
| A7 | protocol.md:98 | CoreRequest "~100 variants" | ~166 | `core.rs:1132` |
| A8 | protocol.md:178 | CoreResponse "~60 variants" | ~110 | `core.rs:542` |
| A9 | protocol.md:235 | CoreEvent "~40 variants" | ~76 | `core.rs:1953` |
| A10 | protocol.md:237 | CoreEvent Snapshot group "(5)" | 6 | Same enum, 6 Snapshot variants |
| A11 | projection.md:138,306 | ProjectionEvent "39 variants" | 46 | `event.rs:128` |
| A12 | jobs.md:126 | JobStore "16 methods" | 21 | `mod.rs:1274` trait definition |
| A13 | tool_broker.md:118 | verify_grant_scope "12 dimensions" | 9 (or clarify pipeline scope) | `broker.rs:172–299` |
| A14 | authorization.md:177 | "153 native operations" | ~160 | `operation_descriptor` match arms |
| A15 | identity.md:181 | "135-row operation matrix" | 157+ | Matches authorization.md count |
| A16 | native_crates.md:154 | codegg-core "27 modules" | 40 | `lib.rs` pub mod declarations |
| A17 | codegg_core.md:20–47 | Module table lists 27 modules | 40 | `lib.rs` (missing 13 modules) |
| A18 | run_store.md:87 | ActualBackend "(9) variants" | 8 | No `Unrouted` variant |
| A19 | lsp.md:810 | server_definitions() "40 servers" | 39 | **See Conflicts section** |
| A20 | skills.md:143 | SourceKind "9 variants" | 10 (missing `Plugin = 35`) | `skills/source.rs` |
| A21 | collaboration.md:49 | "12 CoreRequests" | 18 (3 Presence + 15 Chat) | `core.rs` |
| A22 | collaboration.md:49 | "8 CoreResponses" | 13 (3 Presence + 10 Chat) | `core.rs` |
| A23 | collaboration.md:49 | "4 CoreEvents" | 6 (1 Presence + 5 Chat) | `core.rs` |
| A24 | tts.md:22–24 | Tts struct `speaking: Mutex<AtomicBool>` | `speaking: AtomicBool` (no Mutex) | `src/tts/mod.rs` |
| A25 | tts.md:55 | `init()` return type omitted | Returns `Result<(), AppError>` | `src/tts/mod.rs` |
| A26 | tui.md:405 | "18 state modules" | 22 (missing chat, execution_context, observe, presence) | `src/tui/app/state/` |
| A27 | tui.md:931 | "97 render regression tests" | 99 (`#[test]` annotations) | `tests/tui_render.rs` |
| A28 | testing.md:207–215 | CI Structure lists 7 steps | 8 (missing `check_tui_project_authority.py`) | `.github/workflows/ci.yml` |

### Phantom Types (tool_programs.md)

| # | Doc:Line | Phantom Type | Actual Type |
|---|----------|-------------|-------------|
| A29 | tool_programs.md:80 | `ToolProgramId` | Does not exist in `tool_program/mod.rs` |
| A30 | tool_programs.md:81 | `ProgramCallId` | Does not exist |
| A31 | tool_programs.md:82 | `ToolProgramState` | Lifecycle state in DB/scheduler, not core types |
| A32 | tool_programs.md:83 | `ProgramLanguage` | Constrained by parser, not an enum |
| A33 | tool_programs.md:84 | `ProgramSourceRef` | Does not exist in `store.rs` |
| A34 | tool_programs.md:85 | `ProgramCapabilityManifest` | Does not exist |
| A35 | tool_programs.md:86 | `ProgramCheckpoint` | Actual: `InterpreterCheckpoint` (`interpreter.rs:450`) |
| A36 | tool_programs.md:87 | `ProgramCallRecord` | Actual: `CallRequest`/`CompletedCall` (`interpreter.rs:582,660`) |

---

## B) Wrong-File / Dead / Phantom References

### HIGH

| # | Doc | Claimed File/Ref | Actual Location |
|---|-----|-----------------|-----------------|
| B1 | agent.md:214 | `Agent` at `src/agent/mod.rs:100` | `src/agent/definition.rs:78` |
| B2 | agent.md:239 | `AgentRuntimeKind` at `src/agent/mod.rs:51` | `src/agent/definition.rs:30` |
| B3 | agent.md:291 | `ResolvedAgentExecutionProfile` at `mod.rs:436` | Re-export from `definition.rs` |
| B4 | agent.md:298 | `EMERGENCY_DEFAULT_MODEL` at `mod.rs:402` | `definition.rs:379` |
| B5 | agent.md:386 | `MODEL_ALIAS_FRONTIER` at `mod.rs:397` | `definition.rs:374` |
| B6 | compaction.md:21 | `compact_if_needed()` at `loop.rs:1838` | `src/agent/context_runtime.rs:620` |
| B7 | context-compaction-ownership.md:22 | Same `compact_if_needed` error | Moved to `context_runtime.rs` |
| B8 | cache-aware-context.md:286 | `ContextPolicyConfig` "in src/context/policy.rs" | `codegg_config::schema` crate |
| B9 | git.md:204 | `render_argv` file as `render_argv.rs` | `render.rs` |
| B10 | git.md:233 | `RepoSnapshot` at `git_mutations.rs:246` | `crates/codegg-git/src/workflow.rs:15` |
| B11 | git.md:234 | `StateDelta` at `git_mutations.rs:278` | `crates/codegg-git/src/workflow.rs:30` |
| B12 | git.md:235 | `MutationOutcome` at `git_mutations.rs:324` | `crates/codegg-git/src/workflow.rs:64` |
| B13 | permission.md:393 | `check_external_directory()` | Dead reference — function does not exist |

### MEDIUM

| # | Doc | Issue | Notes |
|---|-----|-------|-------|
| B14 | command_routing.md:129 | `check_kill_switches` at `bash.rs:494–511` | Moved to `src/tool/bash/policy.rs:318–335` |
| B15 | command_routing.md:183 | `DispatchOutcome` at `bash.rs:36–41` | In `src/tool/bash/process.rs:61` |
| B16 | command_intent.md:225 | `classify_command()` at `bash.rs:1671` | `bash.rs` is only 1081 lines; call is in `bash/policy.rs` |
| B17 | git.md:272 | "Canonical source: `process_policy.rs`" | Re-export shim; canonical data in `egggit::process` |

---

## C) Stale Line-Number Updates

Only listings where the batch gives an exact corrected line number. Grouped by doc.

### agent.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `SubAgentReport` | `:30` | `:31` |

### agent-tool-surface.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `apply_tool_exposure_filter()` | `loop.rs` | `request_preparation.rs:15` |
| `Capability` enum | `:14` | `:15` |

### goal.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `Goal` struct | `:51` | `:52` |
| `GoalStatus` | `:6` | `:8` |
| `GoalProgressUpdate` | `:78` | `:81` |
| `CompletionRequest` | `:88` | `:91` |
| `create_active()` | `:157` | `:160` |
| `active_for_session()` | `:209` | `:212` |
| `get()` | `:222` | `:225` |
| `update_status()` | `:231` | `:234` |
| `update_progress()` | `:278` | `:333` |
| `increment_usage()` | `:363` | `:451` |
| `enforce_budget()` | `:424` | `:514` |
| `set_budget()` | `:440` | `:530` |
| `latest_paused_for_session()` | `:469` | `:560` |
| `GoalUpdateProgressTool` | `:71` | `:72` |
| `GoalRequestCompletionTool` | `:187` | `:188` |

### context-ledger.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `ProjectionConfig` | `:22` | `:23` |

### git.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `GitExecutionService` | `git_service.rs:229` | `:232` |
| `GitMutationExecutor` | `git_mutations.rs:703` | `:471` |
| `MutationResult` | `git_mutations.rs:352` | `codegg-git/src/workflow.rs:86` |

### session.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `TuiSessionState` | `session/state.rs:111` | `:112` |

### worktree.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `Worktree (codegg-core)` | `:16` | `:15` |
| `WorktreeInfo (egggit)` | `:5` | `:6` |
| `list_worktrees` | `:33` | `:31` |
| `create_worktree` | `:40` | `:38` |
| `remove_worktree` | `:68` | `:83` |
| `hardened_git_command` | `:98` | `:113` |

### run_store.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `PlannedBackend` | `:111` | `:113` |

### snapshot.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `SnapshotManager` | `:52` | `:56` |

### jobs.md (systematic +4–6 drift)

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `JobId` | `:342` | `:346` |
| `AttemptId` | `:369` | `:373` |
| `ScheduleId` | `:390` | `:394` |
| `DependencyId` | `:411` | `:415` |
| `DaemonGeneration` | `:435` | `:439` |
| `DaemonGeneration::new()` | `:438` | `:442` |
| `JobKind` | `:468` | `:472` |
| `JobSource` | `:548` | `:554` |
| `JobPriority` | `:580` | `:586` |
| `JobPayload` | `:941` | `:947` |
| `JobStore trait` | `:1244` | `:1274` |
| `claim_due` | `schedule_store.rs:519` | `:521` |
| `ResourceRequest::for_kind` | `:644` | `:648` |
| `RecoveryPolicy defaults` | `:1210` | `:1240` |
| `request_cancel` doc | `:1410` | `:1409` |
| `recover_at_startup` | `scheduler.rs:1281` | `:1529` |

### scheduler.md (large drift)

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `main loop` | `scheduler.rs:625` | `:809` |
| `Reconciliation` | `scheduler.rs:435` | `:583` |
| `admit_and_dispatch_batch` | `scheduler.rs:680` | `:833` |
| `JobScheduler` | `scheduler.rs:99` | `:135` |
| `JobSubmissionService` | `submission.rs:83` | `:86` |
| `AdmissionController` | `admission.rs:27` | `:99` (line 27 is `AdmissionDecision`) |
| `shutdown()` | `scheduler.rs:1102` | `:1317` |

### workspace.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `migrate_v22` | `schema.rs:963` | `:1064` |

### protocol.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `EventEnvelope` | `core.rs:125` | `:531` |
| `CoreRequest` | `core.rs:524` | `:1132` |
| `CoreResponse` | `core.rs:137` | `:542` |
| `CoreEvent` | `core.rs:1058` | `:1953` |

### projection.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `ProjectionClientController` | `controller.rs:82` | `:219` |
| `ToolProgramSummary` | `dto.rs:583` | `:822` |
| `ToolProgramDetail` | `dto.rs:708` | `:947` |
| `ToolProgramCallPage` | `dto.rs:679` | `:918` |
| `ProjectionCapabilities` | `caps.rs:39` | `:40` |
| `SessionProjectionSnapshot` | `snapshot.rs:26` | `:29` |
| `ProjectionEvent` | `event.rs:127` | `:128` |
| `ProjectionEnvelope` | `event.rs:53` | `:54` |
| `AppEvent` | `bus/events.rs:60` | `:61` |
| `PermissionDecision` | `bus/mod.rs:11` | `:12` |

### core.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `turn_submit_uses_injected_runtime` test | `daemon.rs:6226` | `:4489` |

### server.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `run_server` | `http.rs:170` | `:182` |

### acp.md (systematic +13 drift)

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `ensure_client` | `acp.rs:288` | `:301` |
| `absolute_cwd` | `acp.rs:304` | `:317` |
| `prompt_text` | `acp.rs:330` | `:343` |
| `native_agents` | `acp.rs:355` | `:368` |
| `replay_snapshot` | `acp.rs:412` | `:425` |
| `handle_event` | `acp.rs:454` | `:467` |
| `event_is_terminal` | `acp.rs:489` | `:502` |
| `cancel_if_ready` | `acp.rs:268` | `:281` |
| acp.md:17 | File "~720 lines" | "~736 lines" |

### permission.md (systematic +8–44 drift)

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `PermissionLevel` | `:115` | `:125` |
| `PermissionResult` | `:133` | `:142` |
| `PermissionDecisionReceipt` | `:145` | `:154` |
| `PermissionChoice` | `:180` | `:189` |
| `ToolRule` | `:226` | `:235` |
| `PermissionRuleset` | `:279` | `:288` |
| `PermissionStore` | `:306` | `:314` |
| `tool_category_for_name()` | `:99` | `:107` |
| `PermissionChecker` | `:489` | `:533` |
| `default_bash_allow_patterns()` | `:1315` | `:1359` |
| `DoomLoopDetector` | `:1574` | `:1618` |

### security.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `classify_bash_command` | `command.rs:193` | `:201` |
| `inspect_text` | `scanner.rs:308` | `:319` |
| `inspect_file` | `scanner.rs:391` | `:402` |

### auth.md (systematic +50–100 drift)

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `AuthConfig` | `auth_types.rs:121` | `:174` |
| `Credential` | `auth_types.rs:61` | `:115` |
| `CredentialKind` | `auth_types.rs:52` | `:54` |
| `AuthError` | `auth_types.rs:14` | `:15` |
| `AuthResolver` | `auth_types.rs:238` | `:298` |
| `ResolverContext` | `auth_types.rs:195` | `:249` |
| `ResolvedAuth` | `auth_types.rs:205` | `:265` |
| `ResolvedAuthSource` | `auth_types.rs:211` | `:271` |
| `CredentialStore` | `auth_types.rs:437` | `:537` |
| `StoredCredentialRecord` | `auth_types.rs:417` | `:517` |
| `ExternalCommandProvider` | `auth_types.rs:176` | `:229` |

### config.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `Config` struct | `schema.rs:203` | `:217` |
| `AuthConfig` | `schema.rs:13` | `:15` |
| `ProviderConfig` | `schema.rs:734` | `:789` |
| `ProviderConnectionsConfig` | `schema.rs:284` | `:339` |
| `ServerConfig` | `schema.rs:686` | `:741` |
| `ProviderConfig::merge()` | `schema.rs:774` | `:827` |

### resilience.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `record_success` | `circuit.rs:156` | `:194` |
| `record_failure` | `circuit.rs:181` | `:219` |
| `CircuitError` | `circuit.rs:14` | `:15` |

### provider.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| Provider trait | `provider_core.rs:50` | `:51` |
| ProviderCapabilities | `provider_core.rs:94` | `:96` |
| `for_provider` | `provider_core.rs:130` | `:131` |
| ChatRequest | `provider_core.rs:172` | `:183` |
| Message | `provider_core.rs:186` | `:199` |
| ContentPart | `provider_core.rs:251` | `:262` |
| ChatEvent | `provider_core.rs:299` | `:311` |
| ToolDefinition | `provider_core.rs:341` | `:353` |
| ModelInfo | `provider_core.rs:389` | `:401` |
| ProviderRegistry | `provider_core.rs:401` | `:412` |
| `register_builtin()` | `provider_core.rs:432` | `:443` |
| `register_builtin_with_config()` | `provider_core.rs:770` | `:844` |
| `create_http_client` | `provider_core.rs:22` | `:23` |
| `EventStream` | `provider_core.rs:34` | `:35` |

### mcp.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `McpService` | `:109` | `:130` |
| `McpClientType` | `:91` | `:112` |
| `McpExposurePolicy` | `:122` | `:144` |
| `OAuthManager` | `:109` | `:142` |
| `TokenSet` | `:76` | `:68` |
| `parse_mcp_tool_server` | `:164` | `:185` |
| `McpEntry` config | `:881` | `:934` |
| `McpServerConfig` | `:889` | `:942` |
| `McpReconnectConfig` | `:906` | `:959` |
| `McpOAuthConfig` | `:914` | `:969` |
| `McpServerStatus` | `:73` | `:74` |
| `McpResource` | `:39` | `:40` |
| `McpResourceContent` | `:47` | `:48` |
| `McpCommand` | `:184` | `:185` |
| `ConnectionState` | `:19` | `:20` |
| `serverInfo/version` | `:163` | `:164` |

### search_backend.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `SearchConfig` | `schema.rs:463` | `:516` |
| `SearchBackendConfig` | `:557` | `:610` |
| `EggsearchConfig` | `:568` | `:621` |
| `ToolTimeoutKind` | `:584` | `:637` |
| `StructuredSearchResult` | `:47` | `:56` |
| `EggsearchCallResult` | `:389` | `:399` |
| `BootstrapReport` | `:275` | `:306` |
| `CrossProcessLockGuard` | `:23` | `:25` |

### hooks.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `HookEvent` | `:15` | `:16` |
| `ShellCommandHook` | `:94` | `:93` |
| `HookRegistry` | `:151` | `:170` |
| `from_config()` | `:167` | `:185` |
| `run_hooks()` | `:193` | `:211` |

### plugin.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `PluginService` | `:20` | `:24` |
| `PluginRuntimeSpec` | `:46` | `:49` |
| `PluginCapability` | `:75` | `:78` |
| `PluginError` | `:530` | `:625` |
| `PluginManager` | `:201` | `:256` |

### ide.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `is_vscode()` | `:83` | `:81` |
| `is_jetbrains()` | `:89` | `:87` |
| `is_ide()` | `:96` | `:94` |
| `open_diff()` | `:100` | `:98` |
| `generate_unified_diff()` | `:392` | `:390` |
| `generate_side_by_side()` | `:420` | `:418` |
| `shutdown()` | `:300` | `:316` |
| `open_diff_handler()` | `:345` | `:361` |
| `parse_file_reference()` | `:372` | `:388` |
| `TempFilesGuard` struct | `:46` | `:42` |
| `register_panic_cleanup()` | `:68` | `:66` |

### skills.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `EffectiveSkill` | `:32` | `:33` |
| `AssetDiscoveryConfig` | `:84` | `:91` |

### memory.md (systematic +2 drift)

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `MemoryStore` | `:70` | `:72` |
| All MemoryStore methods | (consistently +2) | Shift all by +2 |
| `PatternDetector` | `patterns.rs:40` | `:76` |
| `ScoredMemory` | `:269` | `:306` |

### tts.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `toggle_tts`/`stop_tts` | `mod.rs:9820-9921` | `:7583-7640` |

### util.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `tool_interner()` | `src/tool/mod.rs:711` | `src/util/interner.rs:41` |

### tool.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `ToolCategory` | `mod.rs:113–130` | `:115–132` |
| `Tool` trait | `mod.rs:132–201` | `:136–205` |
| `ToolRegistry` | `mod.rs:212–217` | `:215–221` |
| `execute_capture` | `mod.rs:833–865` | `:1036–1068` |
| `ToolCatalog` | `catalog.rs:134–143` | `:170–178` |

### tool_programs.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `BrokerCallback` | `interpreter.rs:638` | `:675` |
| `InterpreterCheckpoint` | `interpreter.rs:449` | `:450` |
| `ProgramResult` | `interpreter.rs:228` | `:229` |

### tool_broker.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `BrokerError` | `broker.rs:946` | `:951` |
| `ToolContract` | `contract.rs:183` | `:184` |

### deterministic_tools.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `build_eggsact_tools` | `deterministic.rs:106–288` | `:106–289` |

### command_planner.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `validate_for_active_routing()` | `plan.rs:431` | `:535` |
| `persist_python_run` | `tool.rs:220` | `:232` |
| file size claim | "5 lines" | "6 lines" |

### test_runner.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `DelegatedTestRun` | `runner.rs:260–272` | `:356–368` |

### python_scripting.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| Diff cap (`MAX_DIFF_CONTENT`) | `executor.rs:669` | `:688` |
| Per-file capture (`MAX_FILE_BYTES`) | `executor.rs:598` | `:617` |

### exec.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| File size | "~298 lines" | "~311 lines" |

### upgrade.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `cmd_upgrade()` | `main.rs:1016–1035` | `:1015` |
| `VersionInfo` | `:7` | `:8` |
| `Config.autoupdate` | `schema.rs:217` | `:229` |
| `AutoupdateConfig` | `schema.rs:192` | `:204` |

### tui.md

| Symbol | Doc Line | Correct Line |
|--------|----------|--------------|
| `App` struct | `mod.rs:865` | `:222` |
| `Component` trait | `component.rs:110` | `:165` |
| `TuiMsg` | `types.rs:86` | `:97` |
| File size claim | "~15,340 lines" | "~13,675 lines" |

---

## D) Structural / Staleness Notes

| # | Doc | Note |
|---|-----|------|
| D1 | git_phase_f_handoff.md | Historical record of Phase F closure — acceptable as historical but references `--all-features` test command (line 111) which contradicts AGENTS.md. Change to `--features server,plugins,lsp-test-support`. |
| D2 | git_polish_verification_handoff.md | Historical record — repeated "Execution-origin matrix" and "Drift guards" sections (lines 252–257 duplicate 235–249). Remove duplicates. |
| D3 | git.md:341 | Duplicate numbering: two items numbered "12" (lines 333, 341). Renumber to 13, 14. |
| D4 | resilience.md | `is_available()` is `#[deprecated]` in source (recommends `call()` for atomic admission). Doc does not mention deprecation. |
| D5 | lsp_disk_cache_threat_model.md | References a `Disk` cache mode that doesn't exist in `LspCacheMode` (only `Disabled` and `Memory`). Prospective design doc — should note this. |
| D6 | git.md:272 | `process_policy.rs` described as "canonical source of truth" but is actually a re-export shim from `egggit::process`. |
| D7 | protocol.md:272 | `EventEnvelope` listed as "Client-to-Server" variant but is actually a bidirectional wrapper. Projection count should be 19, not 17. |
| D8 | config.md:774 | `ProviderConfig::merge()` referenced at line 774 but that line is actually `ServerConfig::merge()`. Correct merge is at line 827. |

---

## E) Improvement Opportunities (Top 15)

| # | Opportunity | Impact | Source Batch |
|---|-------------|--------|--------------|
| E1 | Add a CI test asserting `codegg-core` module count (similar to `built_in_command_count_matches_release_docs`) to prevent 27-vs-40 drift | Prevents module count mismatches | batch7 |
| E2 | Generate `tool_programs.md` Key Types table from actual `pub use` re-exports in `mod.rs` instead of hand-maintaining phantom types | Eliminates 8 phantom types | batch3 |
| E3 | Auto-generate `tool.md` directory tree from `ls src/tool/` or `mod.rs` `pub mod` declarations | Prevents stale directory listings | batch3 |
| E4 | Replace exact line numbers with function-name anchors (e.g. `fn run()` instead of `scheduler.rs:809`) for files >500 lines | Eliminates recurring stale-line maintenance | batch6, batch10 |
| E5 | Batch-update `provider.md` line references with a script that reads actual line numbers from `provider_core.rs` | 14 line refs consistently off by 10–74 | batch7 |
| E6 | Add a `scripts/check_operation_count.py` guard to verify doc's native-operation count against `operation_descriptor` match arms | Prevents authorization count drift | batch8 |
| E7 | Drop exact line references for `schema.rs` config types in mcp.md, search_backend.md, config.md — reference struct names only (schema grows ~50 lines per batch) | Prevents recurring ~53-line offset drift | batch7, batch9 |
| E8 | Extract `ContextPolicyConfig` section from cache-aware-context.md (200+ lines) into its own `architecture/context-policy.md` | Reduces 518-line file; improves navigability | batch1 |
| E9 | Split `lsp.md` Phase 4 typed DTOs section into a separate `lsp_dto.md` | lsp.md is 1000+ lines | batch9 |
| E10 | Add a "last verified" timestamp or freshness check in CI for architecture docs | Prevents silent staleness accumulation | batch2, batch9 |
| E11 | Add `Plugin` source kind (rank 35) to skills.md precedence table and update SourceKind count to 10 | Accuracy — Plugin variant participates in precedence | batch9 |
| E12 | Document that `codegg_core.md` module table should be regenerated from `lib.rs` on each milestone | Prevents 27-vs-40 drift recurring | batch7 |
| E13 | Cross-reference operation counts between authorization.md, identity.md, and collaboration.md using a single source of truth | Eliminates contradictory counts (153 vs 157 vs 160) | batch8, batch9 |
| E14 | Remove `check_external_directory()` dead reference from permission.md and verify if function was removed or never added | Dead content cleanup | batch8 |
| E15 | In `resilience.md`, note that `is_available()` is deprecated and `call()` is the preferred admission path for atomic half-open probe ownership | Clarifies design intent; prevents TOCTOU confusion | batch7 |

---

## Conflicts / Needs Verifier Check

| # | Conflict | Batch Claim | Overview Claim | Notes |
|---|----------|-------------|----------------|-------|
| C1 | **LSP server count** | batch9/lsp.md:810 says **39** | overview.md:284 says **40** | The batches reviewed the actual `server_definitions()` and found 39, but overview asserts 40. One may count differently (e.g., `--` placeholder entries, or a recently-added server). **Needs human verifier to count `server_definitions()` match arms.** |
| C2 | **Tool registration count** | overview.md:281 says **51** tool registrations | No batch directly contradicts this | Consistent — no conflict. |
| C3 | **Eggsact count** | overview.md:283 says **8+5** | batch3/deterministic_tools.md confirms | Consistent. |
| C4 | **AppEvent count** | batch5/bus.md says 53 (doc was wrong, corrected to 53) | overview.md says **53** | Now consistent after batch5 correction. |
| C5 | **Command count** | batch2/command.md confirms **139** | overview.md says **139** | Consistent. |
| C6 | **Agent count** | No batch disputes | overview.md says **10** | Consistent. |
| C7 | **Table count** | batch4/session.md said 52+ (wrong); overview says **71** | overview verified against `schema.rs` | Consistent after batch4 correction. |
| C8 | **Storage layout** | No batch disputes | overview.md says **56** | Consistent. |
| C9 | **Test files** | No batch disputes | overview.md says **189** | Consistent. |
| C10 | **Doc count** | No batch disputes | overview.md says **77** | Consistent. |
| C11 | **GitOperation count** | batch4/git.md said 12 (wrong); overview says **54** | overview verified | Consistent after batch4 correction. |
| C12 | **GitRiskClass count** | No batch disputes | overview.md says **11** | Consistent. |
| C13 | **Provider count** | No batch disputes | overview.md says **15** | Consistent. |
| C14 | **Guard count** | No batch disputes | overview.md says **21** | Consistent. |
| C15 | **Native operations count** | batch8/authorization.md says ~160 (doc was 153, corrected) | overview.md does not list this count | No conflict with overview, but batch8/identity.md says 157 — these need reconciliation (authorization.md native count vs identity.md matrix count are measuring different things). |

---

## Per-Doc Fix Checklist

Fix agents should work doc by doc. The doc name is followed by the issue count and severity breakdown.

### Batch 1 — Agent & Context

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| agent.md | 6 | 5 | 0 | 1 | A1, B1–B5; line ref for SubAgentReport (C) |
| agent-tool-surface.md | 2 | 0 | 0 | 2 | Line refs for apply_tool_exposure_filter, Capability (C) |
| compaction.md | 1 | 1 | 0 | 0 | B6: compact_if_needed wrong file |
| context-compaction-ownership.md | 1 | 1 | 0 | 0 | B7: same compact_if_needed error |
| cache-aware-context.md | 2 | 2 | 0 | 0 | A2 (field count), B8 (config struct file) |
| context-ledger.md | 1 | 0 | 0 | 1 | ProjectionConfig line (C) |
| model_profile_task_state.md | 0 | — | — | — | No issues found |
| goal.md | 13 | 0 | 0 | 13 | All line refs stale (C: GoalStore table) |
| research.md | 1 | 0 | 0 | 1 | "15 files" count wrong → 14 + sources/ |

### Batch 2 — Command Execution

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| command.md | 0 | — | — | — | Clean |
| command_intent.md | 1 | 0 | 1 | 0 | B16: classify_command wrong file |
| command_planner.md | 3 | 0 | 0 | 3 | Line refs (C: validate_for_active_routing, persist_python_run, file size) |
| command_routing.md | 2 | 0 | 2 | 0 | B14, B15: check_kill_switches, DispatchOutcome wrong files |
| exec.md | 1 | 0 | 0 | 1 | File size claim |
| test_runner.md | 1 | 0 | 0 | 1 | DelegatedTestRun line (C) |
| python_scripting.md | 2 | 0 | 0 | 2 | Diff cap, MAX_FILE_BYTES lines (C) |

### Batch 3 — Tools

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| tool.md | 5 | 0 | 0 | 5 | Line refs: ToolCategory, Tool trait, ToolRegistry, execute_capture, ToolCatalog (C) |
| tool_broker.md | 3 | 1 | 1 | 1 | A13: 12→9 dimensions; BrokerError, ToolContract lines (C) |
| tool_programs.md | 11 | 9 | 0 | 2 | A29–A36: 8 phantom types; BrokerCallback/Checkpoint/Result lines (C) |
| tool_program_language.md | 0 | — | — | — | Clean |
| deterministic_tools.md | 1 | 0 | 0 | 1 | build_eggsact_tools line +1 (C) |
| preflight.md | 0 | — | — | — | Clean (all verified correct) |

### Batch 4 — Git & Persistence

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| git.md | 9 | 5 | 3 | 1 | A3: GitPayload 12→13; B9–B12: wrong files; duplicate numbering D3 |
| git_phase_f_handoff.md | 1 | 0 | 1 | 0 | D1: --all-features command |
| git_polish_verification_handoff.md | 1 | 0 | 1 | 0 | D2: duplicate sections |
| session.md | 3 | 2 | 0 | 1 | A4: 52+→71 tables; A5: 22→27 columns; TuiSessionState line (C) |
| storage.md | 0 | — | — | — | Clean |
| snapshot.md | 1 | 0 | 1 | 0 | SnapshotManager line (C) |
| worktree.md | 6 | 0 | 2 | 4 | Line refs (C: Worktree, WorktreeInfo, list/create/remove/hardened_git) |
| run_store.md | 2 | 0 | 1 | 1 | A18: ActualBackend 9→8; PlannedBackend line (C) |

### Batch 5 — Core & Transport

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| core.md | 1 | 0 | 1 | 0 | Test line off by 1737 (C) |
| server.md | 1 | 0 | 0 | 1 | run_server line (C) |
| client.md | 0 | — | — | — | Clean |
| acp.md | 10 | 0 | 0 | 10 | All function lines +13 systematic (C) + file size claim |
| protocol.md | 10 | 5 | 0 | 5 | A7–A10: variant counts + Snapshot group; line refs (C) |
| projection.md | 11 | 1 | 1 | 9 | A11: 39→46 variants; line refs for controller/dto/event/snapshot/caps (C) |
| bus.md | 3 | 1 | 0 | 2 | A6: 45→53; line refs (C: AppEvent, PermissionDecision) |

### Batch 6 — Workspace & Jobs

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| workspace.md | 1 | 1 | 0 | 0 | Schema migration v22 line (C: 963→1064) |
| workspace_services.md | 0 | — | — | — | Clean |
| jobs.md | 17 | 1 | 2 | 14 | A12: 16→21 methods; systematic +4–6 line drift (C) |
| scheduler.md | 7 | 5 | 1 | 1 | Lines off by 148–248 for main loop, reconcile, admit, controller, shutdown (C) |
| process-tool-execution-ownership.md | 0 | — | — | — | Clean |
| project_catalog.md | 0 | — | — | — | Clean |
| project_identity_storage.md | 0 | — | — | — | Clean |

### Batch 7 — Provider & Config

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| provider.md | 14 | 0 | 1 | 13 | All provider_core.rs lines consistently off +10–74 (C) |
| model-adapters.md | 1 | 0 | 0 | 1 | Verify 7 adapter count |
| config.md | 7 | 1 | 4 | 2 | B: ProviderConfig::merge wrong line; lines for Config/AuthConfig/ProviderConfig/etc (C) |
| resilience.md | 5 | 0 | 2 | 3 | D4: is_available() deprecated; record_success/failure lines (C) |
| error.md | 0 | — | — | — | All verified correct |
| native_crates.md | 2 | 1 | 0 | 1 | A16: 27→40 modules |
| codegg_core.md | 1 | 1 | 0 | 0 | A17: module table 27→40 |

### Batch 8 — Security

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| permission.md | 12 | 1 | 0 | 11 | B13: dead check_external_directory(); all lines +8–44 systematic (C) |
| authorization.md | 1 | 1 | 0 | 0 | A14: 153→160 native operations |
| audit.md | 0 | — | — | — | Clean |
| security.md | 4 | 0 | 0 | 4 | classify_bash_command, inspect_text, inspect_file lines (C) |
| crypto.md | 0 | — | — | — | Clean |
| auth.md | 11 | 0 | 0 | 11 | All auth_types.rs lines +50–100 systematic (C) |
| identity.md | 1 | 1 | 0 | 0 | A15: 135-row matrix → 157+ |
| lsp_disk_cache_threat_model.md | 1 | 0 | 1 | 0 | D5: Disk mode doesn't exist — prospective doc |

### Batch 9 — Integrations

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| mcp.md | 16 | 0 | 11 | 5 | All lines +1–55 (schema.rs growth); see C1 for LSP count |
| lsp.md | 1 | 1 | 0 | 0 | A19: 40→39 servers (**conflict with overview**) |
| plugin.md | 5 | 0 | 2 | 3 | PluginService/RuntimeSpec/Capability/Error/Manager lines (C) |
| hooks.md | 5 | 0 | 3 | 2 | HookEvent/ShellCommandHook/HookRegistry/from_config/run_hooks (C) |
| skills.md | 2 | 1 | 0 | 1 | A20: 9→10 SourceKind variants; EffectiveSkill/AssetDiscoveryConfig lines (C) |
| search_backend.md | 8 | 0 | 7 | 1 | All schema.rs lines +53 (C); StructuredSearchResult, EggsearchCallResult, BootstrapReport, CrossProcessLockGuard |
| ide.md | 11 | 0 | 3 | 8 | All lines +2–16 systematic (C) |
| collaboration.md | 3 | 3 | 0 | 0 | A21–A23: CoreRequest/Response/Event counts all wrong |
| presence.md | 0 | — | — | — | Clean |

### Batch 10 — TUI & Support

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| tui.md | 6 | 3 | 0 | 3 | A26: 18→22 state modules; A27: 97→99 tests; B: App struct line (C) |
| theme.md | 0 | — | — | — | Clean |
| human_shell.md | 0 | — | — | — | Clean |
| memory.md | 4 | 0 | 0 | 4 | PatternDetector/ScoredMemory lines off by 36–37; systematic +2 (C) |
| tts.md | 4 | 2 | 0 | 2 | A24–A25: Mutex<AtomicBool> wrong; init() return type; toggle_tts/stop_tts lines (C) |
| upgrade.md | 4 | 0 | 0 | 4 | cmd_upgrade/VersionInfo/Config.autoupdate/AutoupdateConfig lines (C) |
| util.md | 1 | 0 | 0 | 1 | tool_interner() wrong file and line (C) |
| testing.md | 1 | 1 | 0 | 0 | A28: CI Structure missing TUI authority guard step |

### Overview

| Doc | Total | HIGH | MEDIUM | LOW | Key Fixes |
|-----|-------|------|--------|-----|-----------|
| overview.md | 0 | — | — | — | Clean — serves as the verified-count baseline |

---

## Totals by Doc

| Doc | HIGH | MEDIUM | LOW |
|-----|------|--------|-----|
| agent.md | 5 | 0 | 1 |
| agent-tool-surface.md | 0 | 0 | 2 |
| compaction.md | 1 | 0 | 0 |
| context-compaction-ownership.md | 1 | 0 | 0 |
| cache-aware-context.md | 2 | 0 | 0 |
| context-ledger.md | 0 | 0 | 1 |
| goal.md | 0 | 0 | 13 |
| research.md | 0 | 0 | 1 |
| command_intent.md | 0 | 1 | 0 |
| command_planner.md | 0 | 0 | 3 |
| command_routing.md | 0 | 2 | 0 |
| exec.md | 0 | 0 | 1 |
| test_runner.md | 0 | 0 | 1 |
| python_scripting.md | 0 | 0 | 2 |
| tool.md | 0 | 0 | 5 |
| tool_broker.md | 1 | 1 | 1 |
| tool_programs.md | 9 | 0 | 2 |
| deterministic_tools.md | 0 | 0 | 1 |
| git.md | 5 | 3 | 1 |
| git_phase_f_handoff.md | 0 | 1 | 0 |
| git_polish_verification_handoff.md | 0 | 1 | 0 |
| session.md | 2 | 0 | 1 |
| snapshot.md | 0 | 1 | 0 |
| worktree.md | 0 | 2 | 4 |
| run_store.md | 0 | 1 | 1 |
| core.md | 0 | 1 | 0 |
| server.md | 0 | 0 | 1 |
| acp.md | 0 | 0 | 10 |
| protocol.md | 5 | 0 | 5 |
| projection.md | 1 | 1 | 9 |
| bus.md | 1 | 0 | 2 |
| workspace.md | 1 | 0 | 0 |
| jobs.md | 1 | 2 | 14 |
| scheduler.md | 5 | 1 | 1 |
| provider.md | 0 | 1 | 13 |
| config.md | 1 | 4 | 2 |
| resilience.md | 0 | 2 | 3 |
| native_crates.md | 1 | 0 | 1 |
| codegg_core.md | 1 | 0 | 0 |
| permission.md | 1 | 0 | 11 |
| authorization.md | 1 | 0 | 0 |
| security.md | 0 | 0 | 4 |
| auth.md | 0 | 0 | 11 |
| identity.md | 1 | 0 | 0 |
| lsp_disk_cache_threat_model.md | 0 | 1 | 0 |
| mcp.md | 0 | 11 | 5 |
| lsp.md | 1 | 0 | 0 |
| plugin.md | 0 | 2 | 3 |
| hooks.md | 0 | 3 | 2 |
| skills.md | 1 | 0 | 1 |
| search_backend.md | 0 | 7 | 1 |
| ide.md | 0 | 3 | 8 |
| collaboration.md | 3 | 0 | 0 |
| tui.md | 3 | 0 | 3 |
| memory.md | 0 | 0 | 4 |
| tts.md | 2 | 0 | 2 |
| upgrade.md | 0 | 0 | 4 |
| util.md | 0 | 0 | 1 |
| testing.md | 1 | 0 | 0 |
| **Totals** | **38** | **52** | **74** |
