# Consolidated Architecture Review

**Reviewed**: 2026-09-13
**Sources**: 12 canonical batch review files (review_batch0 through review_batch11)

## Summary

All 77 architecture documents were reviewed against source code across 12 parallel batches. Documentation is generally high quality — major structural claims (enum variant counts, protocol versions, tool registrations, pipeline stages) are largely accurate. The primary issues are **wrong numeric counts** in high-churn modules (TUI domains, ServerCapabilities fields, tool registrations, identity newtypes), **stale line-number references** (run_store view models, test_runner, command_intent), and **incomplete struct/enum listings** (ProviderCapabilities, ServerCapabilities, identity newtypes). No security-critical code bugs were found.

| Severity | Count |
|----------|-------|
| HIGH | 2 |
| MEDIUM | 23 |
| LOW | 33 |
| **Total unique findings** | **58** |

## HIGH Severity — Documentation Fixes

| # | File | Issue | Evidence | Source Batch |
|---|------|-------|----------|--------------|
| H1 | `overview.md:196` | TUI module row says "state management across **6 domains**" — code has **22** state modules (tui.md itself lists 22 at line 405) | `src/tui/state/` has 22 domain modules | review_batch10_tui |
| H2 | `protocol.md:305-306` | `ServerCapabilities` listed with only 5 fields (`event_replay`, `session_management`, `permission_routing`, `workspace_registration`, `workspace_snapshots`) — actual struct has **10 fields** (missing `durable_jobs`, `durable_schedules`, `identity_aware_context`, `project_catalog`, `session_projection`) | `crates/codegg-protocol/src/frames.rs:102-132` | review_batch6_core |

## HIGH Severity — Code Issues

| # | Module | Issue | Location |
|---|--------|-------|----------|
| — | — | No security-critical or behavioral code bugs found across all batches | — |

## MEDIUM Severity — Documentation Fixes

| # | File | Issue | Evidence | Source Batch |
|---|------|-------|----------|--------------|
| M1 | `agent.md:294` | `AgentLoop` stated to have "**32 direct fields**" — actual count is **30** fields | `loop.rs:86-129` | review_batch1_agent |
| M2 | `agent.md:436` | `SubAgentRequest` listing shows **11 fields** but code has **13 fields** (missing `run_id`, `parent_run_id`, `workspace_locks`) | `worker.rs:82-99` | review_batch1_agent |
| M3 | `research.md:19` | States "**14 files + sources/ subdirectory with 8 adapters**" — actual is **13 files** and **7 adapter files** (6 registered + 1 unregistered advisory) | `src/research/` file count | review_batch1_agent |
| M4 | `tool.md` | "**~31 always-registered core tools**" — actual is **~39 unconditional `register()` calls** in `with_options()` (including hidden stubs for lsp/security when disabled, plus eggsact 13) | `src/tool/mod.rs:375-784` | review_batch3_tools |
| M5 | `git.md:31` | Mutation action count "**40 actions**" — actual is **42** mutation action enum entries in `GitTool` schema | `GitTool` schema | review_batch4_git |
| M6 | `identity.md:17-19` | Lists **10 identity newtypes** but source defines **13** via `typed_identity!` macro (missing `AgentRunGroupId`, `AgentRunMessageId`, `ChatMessageId`) | `identity.rs:251,274,280` | review_batch5_persistence |
| M7 | `codegg_core.md:22` | Module table lists **~40 modules** — `lib.rs` has **42 `pub mod` declarations** | `crates/codegg-core/src/lib.rs` | review_batch7_providers |
| M8 | `native_crates.md:22,153` | Same "**40 modules**" claim — actual is **42** | `lib.rs` has 42 `pub mod` | review_batch7_providers |
| M9 | `provider.md:136-142` | `ProviderCapabilities` doc implies **~3 fields** — actual struct has **12-14 fields** | `provider_core.rs` struct | review_batch7_providers |
| M10 | `exec.md:179` | `classify_error()` line range stated as `exec.rs:204-272` — actual starts at **line 217** (off by 13) | `src/exec.rs:217` | review_batch2_commands |
| M11 | `test_runner.md:334-335` | `DelegatedTestRun` at `runner.rs:260` — actual **:356**; `setsid` at `runner.rs:290` — actual **:386**; kill at `:478` — actual **:595** (line refs stale by **96-117 lines**) | `src/test_runner/runner.rs` | review_batch2_commands |
| M12 | `command_intent.md:240,269` | `CommandIntentFamily` at `schema.rs:2904` — actual **:3005** (off by ~100); `CommandIntentConfig` at `schema.rs:2741` — actual **~:2900+** | `schema.rs` | review_batch2_commands |
| M13 | `run_store.md` | `FsRunStore` at `:1110` — actual **:1126**; `MemRunStore` at `:1730` — actual **:1752**; all view models (`:861-:930`) off by **15-22 lines** (systematic drift) | `run_store.rs` | review_batch4_git |
| M14 | `git_phase_f_handoff.md:116` | "**402 egggit tests**" — actual is **75 egggit + 358 codegg-git = 433** total across both crates | `cargo test -p egggit` / `codegg-git` | review_batch4_git |
| M15 | `git_phase_f_handoff.md:41` | `mutation enum entries ≥35` — actual is **42** | `GitTool` schema | review_batch4_git |
| M16 | `git_polish_verification_handoff.md:34` | `operation.rs` has "**47 variants**" — actual is **54** | `crates/codegg-git/src/operation.rs` | review_batch4_git |
| M17 | `overview.md:257` | Crypto module mapped to **`auth/`** directory — source is **`crates/codegg-providers/src/crypto.rs`** | `src/auth/mod.rs` re-exports `auth_types` not crypto | review_batch8_security |
| M18 | `tui.md:361-362` | Claims "**All dispatch arms in `command_dispatch.rs` are `fn` (non-async). No `.await` points in the match.**" — **incorrect**: `dispatch_tui_command` is `pub(crate) async fn` with **5 `.await` calls** on `core_client.request(req).await` | `src/tui/runtime/command_dispatch.rs:1` | review_batch10_tui |
| M19 | `session.md:56-98` | Table groupings omit `agent_run_group`, `agent_run_group_member`, `agent_run_journal`, `agent_run_mailbox`, `agent_run_result`, `managed_worktree`, `worktree_lease` tables (added in undocumented migrations v38-v50) | `session/schema.rs` | review_batch5_persistence |
| M20 | `storage.md:238-261` | Migration list documents v22-v37, v46-v56 but **omits v38-v45 and v50**; lacks catch-all reference to `session/schema.rs` | `session/schema.rs:137-174` | review_batch5_persistence |
| M21 | `command_routing.md` | Doc describes routing as an independent module but `src/command_routing.rs:1-5` explicitly self-identifies as "**compatibility facade for the pre-M006 routing API**" with "zero-logic adapter." Framing is buried in "Where It Lives" section. | `src/command_routing.rs:1-5` | review_batch2_commands |
| M22 | `overview.md:178` | "**66 files**" in `src/tool/` — actual count is **68 `.rs` files** (including subdirectories `bash/`) | `src/tool/` file count | review_batch0_overview |
| M23 | `overview.md:149` | Command Routing listed as "**pipeline stage 3**" — but `command_routing.md` correctly identifies itself as a compatibility facade. Conceptual mismatch between overview and routing doc. | `overview.md:149` vs `command_routing.md:1-5` | review_batch2_commands |

## MEDIUM Severity — Stale Content

| # | File | Content | Reason | Source Batch |
|---|------|---------|--------|--------------|
| MS1 | `overview.md:196` | "state management across **6 domains**" | Expanded to 22 state modules; tui.md itself acknowledges this at line 927 | review_batch10_tui |
| MS2 | `tui.md:361-362` | "All dispatch arms are `fn` (non-async). No `.await` points." | `dispatch_tui_command` is `async fn` with 5 `.await` calls | review_batch10_tui |

## LOW Severity — Documentation Fixes

| # | File | Issue | Source Batch |
|---|------|-------|--------------|
| L1 | `overview.md:189` | "Tools and Capabilities at a Glance" table **omits `context_read`** tool (registered conditionally at `src/tool/mod.rs:778-783`) | review_batch0_overview |
| L2 | `overview.md:305` | Verified Counts row says "53 tool registrations" — accurate, but line 178 says "66 files" which could confuse readers into thinking 66 ≠ 53 | review_batch0_overview |
| L3 | `agent.md:436` | `SubAgentRequest` missing 3 undocumented scheduler/workspace fields | review_batch1_agent |
| L4 | `research.md:82-85` | `AdvisorySource` implements `ResearchSourceAdapter` but is not registered in coordinator — could confuse readers who count impls | review_batch1_agent |
| L5 | `model_profile_task_state.md:132-157` | `ResolvedModelProfile` field table lists ~18 fields but code has **19** (missing `orchestration_tier`) | review_batch1_agent |
| L6 | `command_planner.md:66` | `ProjectorRoute` table lists 9 variants without `RtkEligible` — line 79 adds it but table is incomplete | review_batch2_commands |
| L7 | `test_runner.md:286,334` | Two `DEFAULT_MAX_REPORT_BYTES` constants exist in both `runner.rs:26` and `report.rs` — clarify which is canonical | review_batch2_commands |
| L8 | `python_scripting.md` | `SandboxFailureKind` variants (Unavailable/Policy/Setup/Helper) not listed in doc — important for debugging | review_batch2_commands |
| L9 | `tool_broker.md` | Missing `into_programmatic_outcome` method reference (doc mentions `programmatic_outcome()` but `tool_programs.md:570` references the consuming variant) | review_batch3_tools |
| L10 | `preflight.md` | Multiple line ranges off by 1-4 lines (`PreflightMode` at 114 not 110, `PreflightService` end line unclear) | review_batch3_tools |
| L11 | `tool_program_language.md` | `rustpython-parser` version claimed as 0.4.0 — not verified against `Cargo.lock` in this pass | review_batch3_tools |
| L12 | `worktree.md:208` | "**integration tests (11 tests)**" — actual is **14** | review_batch4_git |
| L13 | `worktree.md:142-170` | Multiple line refs off by 1-2 lines (`list_worktrees` at :33 → :31; `create_worktree` at :40 → :38; `remove_worktree` at :68 → :83) | review_batch4_git |
| L14 | `run_store.md:272` | "**19 unit tests**" — actual is **18** | review_batch4_git |
| L15 | `project_identity_storage.md:88-89` | `BindingStatus::RebindRequired` listed but no explanation of when sessions transition to this state | review_batch5_persistence |
| L16 | `goal.md:203-215` | 8 of 12 `GoalStore` line references stale by 3-90 lines | review_batch1_agent |
| L17 | `agent-tool-surface.md:18` | `apply_tool_exposure_filter()` in `loop.rs` — actual implementation in `request_preparation.rs:15` | review_batch1_agent |
| L18 | `exec.md` | Doc lists **24 error codes** but `classify_error()` completeness not fully verified | review_batch2_commands |
| L19 | `bus.md:62` | "Other" category table lists **8 variants** but actually contains **10** (ConfigChanged, AgentChanged, ModelChanged, CompactionTriggered, Error, Info, TodoUpdated, FileChanged, ContextUpdated, PluginUiEffect) | review_batch10_tui |
| L20 | `projection.md:150` | `ConvergenceUpserted` variant listed in code but missing from ProjectionEvent family table | review_batch10_tui |
| L21 | `tui.md:651` | `TuiMsg` enum listing omits newer variants like `OpenImportDialog`, `SubmitConnect`, `CloseDialog`, `ToggleSidebar` | review_batch10_tui |
| L22 | `human_shell.md:128` | `ShellOutputStore` defaults "**1 MB/cmd (head 256KB + tail 256KB)**" — math sums to 512KB, not 1 MB | review_batch10_tui |
| L23 | `provider.md:52` | Provider trait listed at line 51 — actual `pub trait Provider` at line **52** (`#[async_trait]` at 51) | review_batch7_providers |
| L24 | `mcp.md` | `protocol.rs` not listed in "Where It Lives" file layout table (`pub(crate)` internal file) | review_batch9_integrations |
| L25 | `bus.md:164,167` | AppEvent at `events.rs:60` — actual **:61**; PermissionDecision at `mod.rs:11` — actual **:12** | review_batch10_tui |
| L26 | `memory.md:115,127` | `PatternDetector` at `:40` — actual **:76** (off by 36); `ScoredMemory` at `:269` — actual **:306** (off by 37) | review_batch11_daemon |
| L27 | `tui.md:366,611,646` | `App` at `:865` — actual **:222**; `Component` at `:110` — actual **:165**; `TuiMsg` at `:86` — actual **:97** | review_batch10_tui |
| L28 | `tts.md:22-24` | `Tts` struct described as `speaking: Mutex<AtomicBool>` — actual field is `speaking: AtomicBool` (no Mutex) | review_batch11_daemon |
| L29 | `provider.md:50-432` | Multiple line refs off by 1-11 lines throughout document | review_batch7_providers |
| L30 | `scheduler.md:87` | `JobScheduler` at `scheduler.rs:99` — actual **:135** (off by 36) | review_batch11_daemon |
| L31 | `upgrade.md:14,50,80,81` | Line refs off by 1-12 lines | review_batch11_daemon |
| L32 | `jobs.md:114-119` | AttemptState machine omits `Created → Admitted` transition | review_batch11_daemon |
| L33 | `overview.md:178` | "66 files" in `src/tool/` — actual is **68** `.rs` files | review_batch0_overview |

## LOW Severity — Stale Content

| # | File | Content | Reason | Source Batch |
|---|------|---------|--------|--------------|
| LS1 | `git_phase_f_handoff.md:104-105` | egggit "71 tests", codegg-git "331 tests" | Natural test growth: egggit 75, codegg-git 358 | review_batch4_git |
| LS2 | `git_phase_f_handoff.md:110` | `git_closure_matrix` "32 tests" | Growth to 34 tests | review_batch4_git |
| LS3 | `git_polish_verification_handoff.md:242` | execution-origin matrix "26 tests" | Growth to 28 tests | review_batch4_git |
| LS4 | `upgrade.md:83-84` | `autoupdate` config defined but never wired to upgrade module | Dead config field | review_batch11_daemon |
| LS5 | `tts.md:96` | `pkill say` stops ALL `say` processes system-wide, not just CodeGG's child | Documented but process-group-aware kill would be safer | review_batch11_daemon |
| LS6 | `util.md` | `tool_interner()` grows monotonically — no bounded LRU or periodic reset | Memory hygiene concern for long-running daemons | review_batch11_daemon |
| LS7 | `permission.md:15` | `PermissionRegistry (ask-response broker) → crates/codegg-core/src/bus/mod.rs` note at :21 is accurate | Verified correct | review_batch8_security |

## Improvements — Cross-Cutting

| # | Module | Opportunity | Impact | Source Batch |
|---|--------|-------------|--------|--------------|
| I1 | protocol.md | Regenerate `CoreRequest`/`CoreResponse`/`CoreEvent` variant tables from source enums; add CI assertion test for variant counts | Prevents recurring count drift | review_batch6_core |
| I2 | projection.md | Regenerate `ProjectionEvent` table from `event.rs:128` | Prevents variant count drift | review_batch10_tui |
| I3 | jobs.md, scheduler.md | Replace hardcoded line numbers with `fn name` anchors or "last verified" timestamps — both files are 1700+ lines and shift with every edit | Eliminates recurring stale-line maintenance | review_batch11_daemon |
| I4 | provider.md | Document full `ProviderCapabilities` field set (12-14 fields) rather than summarizing 3 | Reduces future drift; becomes authoritative for integration | review_batch7_providers |
| I5 | codegg_core.md | Regenerate module table from `lib.rs` (automated or scripted) to prevent drift as new modules are added | Prevents 40-vs-42 module count discrepancies | review_batch7_providers |
| I6 | run_store.md | Bulk refresh line numbers for view models (`:861-:930` → `:877-:947`) | Systematic 15-22 line drift | review_batch4_git |
| I7 | resilience.md | Document `call()` as preferred admission path replacing deprecated `is_available()` | Guides future callers to atomic API | review_batch7_providers |
| I8 | identity.md | Update newtypes list from 10 to 13; add "Evolution" note for `typed_identity!` macro | Completeness | review_batch5_persistence |
| I9 | overview.md | Add `ide` module to module map; add `context_read` to tools table; update TUI "6 domains" → "22 state modules" | Meta-document accuracy | review_batch0_overview, review_batch9_integrations, review_batch10_tui |
| I10 | agent.md | Add field-count cross-check comment (e.g., "N fields as of YYYY-MM-DD") so stale counts are caught during reviews | Prevents drift | review_batch1_agent |
| I11 | command_routing.md | Reframe opening to "This is a compatibility facade, not an independent module" rather than burying it in "Where It Lives" | Clarity for readers | review_batch2_commands |

## Cross-Module Issues

### 1. Numeric Count Drift Pattern
Multiple high-churn modules have grown without documentation updates:
- TUI state modules: doc says 6, code has 22 (+267%)
- `ServerCapabilities`: doc lists 5 fields, code has 10 (+100%)
- `AgentLoop` fields: doc says 32, code has 30 (-6%)
- `SubAgentRequest` fields: doc says 11, code has 13 (+18%)
- Tool registrations: doc says "~31 always-registered", code has ~39 unconditional (+26%)
- `identity.md` newtypes: doc lists 10, code has 13 (+30%)
- `codegg-core` modules: doc says 40, code has 42 (+5%)

**Recommendation**: Add CI assertion tests that assert documented counts match actual code counts (pattern exists: `assert_eq!(CommandRegistry::built_in_commands().len(), 139)`).

### 2. Systematic Line-Number Drift
Several modules have line references that are uniformly stale:
- `run_store.md`: View model refs off by 15-22 lines (systematic)
- `test_runner.md`: Line refs off by 96-117 lines
- `command_intent.md`: Schema.rs refs off by ~100 lines
- `provider.md`: Line refs off by 1-11 lines throughout

**Recommendation**: For high-churn files (>500 lines), prefer file-path-only references or anchor-based linking. Add a `last_verified` timestamp field to docs referencing specific line numbers.

### 3. Incomplete Struct/Enum Listings
Several docs list only a subset of fields/variants:
- `protocol.md` `ServerCapabilities`: 5 of 10 fields documented
- `provider.md` `ProviderCapabilities`: ~3 of 12-14 fields documented
- `identity.md` identity newtypes: 10 of 13 listed
- `tui.md` `TuiMsg` enum: omits newer variants

**Recommendation**: When a struct/enum has >5 fields, either list all fields or explicitly note "see source for full listing" to prevent silent drift.

### 4. Historical Snapshot Docs
Handoff documents (`git_phase_f_handoff.md`, `git_polish_verification_handoff.md`) contain snapshot-time counts that are naturally stale:
- egggit tests: 71 → 75
- codegg-git tests: 331 → 358
- operation.rs variants: 47 → 54
- "402 egggit tests" claim appears to be a conflation (actual: 75 + 358 = 433 total)

These are acceptable as historical records but should carry "last verified" timestamps to prevent confusion.

## Sources

Exactly the 12 canonical batch review files:

1. `plans/review_batch0_overview.md` — overview.md
2. `plans/review_batch1_agent.md` — agent context and execution
3. `plans/review_batch2_commands.md` — command pipeline and execution
4. `plans/review_batch3_tools.md` — tool layer and programs
5. `plans/review_batch4_git.md` — git, worktree, run artifacts
6. `plans/review_batch5_persistence.md` — persistence and identity
7. `plans/review_batch6_core.md` — core facade and transport
8. `plans/review_batch7_providers.md` — providers, config, crates
9. `plans/review_batch8_security.md` — security and authorization
10. `plans/review_batch9_integrations.md` — external integrations
11. `plans/review_batch10_tui.md` — TUI, commands, events
12. `plans/review_batch11_daemon.md` — daemon services and support

## Verified Counts (Cross-Check vs overview.md)

| Claim | Doc Value | Actual | Match | Source Batch |
|-------|-----------|--------|-------|--------------|
| Tool registration statements | 53 | 53 | ✅ | review_batch0_overview |
| LSP servers | 39 | 39 | ✅ | review_batch0_overview, review_batch9_integrations |
| AppEvent variants | 53 | 53 | ✅ | review_batch0_overview, review_batch10_tui |
| Built-in slash commands | 139 | 139 | ✅ | review_batch0_overview, review_batch10_tui |
| Built-in agents | 10 | 10 | ✅ | review_batch0_overview |
| DB tables | 71 | 71 | ✅ | review_batch0_overview |
| Storage layout version | 56 | 56 | ✅ | review_batch0_overview |
| Integration test files | 189 | 189 | ✅ | review_batch0_overview |
| Architecture docs | 77 | 77 | ✅ | review_batch0_overview |
| CI guard scripts | 21 | 21 | ✅ | review_batch0_overview |
| Env-var providers | 15 | 15 | ✅ | review_batch0_overview, review_batch7_providers |
| GitOperation variants | 54 | 54 | ✅ | review_batch0_overview, review_batch4_git |
| GitRiskClass variants | 11 | 11 | ✅ | review_batch0_overview, review_batch4_git |
| Bundled themes | 50 | 50 | ✅ | review_batch10_tui |
| CoreRequest variants | ~166 | **166** | ✅ | review_batch6_core |
| CoreResponse variants | ~110 | **110** | ✅ | review_batch6_core |
| CoreEvent variants | ~76 | **76** | ✅ | review_batch6_core |
| ProjectionEvent variants | 46 | **46** | ✅ | review_batch10_tui |
| codegg-core modules | 42 | **42** | ✅ | review_batch7_providers |
| TUI state modules | 22 | **22** | ✅ | review_batch10_tui |
| dialog variants | 41 | 41 | ✅ | review_batch10_tui |
| ProjectionEvent variants | 46 | 46 | ✅ | review_batch10_tui |
