# Consolidated Architecture Review

**Reviewed**: 2026-09-13
**Sources**: 12 batch review files (batch0–batch11) plus archived batch8, covering all 77 architecture documents

## Summary

All 77 architecture documents were reviewed against source code across 12 parallel batches. The documentation is in good overall shape — major structural claims (enum variant counts, protocol versions, pipeline stages, tool registrations) are largely accurate. The primary problems are **stale line-number references** (many off by 10–900 lines due to refactoring), **incomplete variant/field counts** in high-churn modules (protocol, projection, codegg-core), and a few **wrong file paths** where functions were moved between modules during refactoring. No security-critical divergences were found.

| Severity | Count |
|----------|-------|
| HIGH | 15 |
| MEDIUM | 22 |
| LOW | 28 |
| **Total unique findings** | **65** |

## HIGH Severity — Documentation Fixes

| # | File | Issue | Evidence | Source Batch |
|---|------|-------|----------|--------------|
| H1 | `protocol.md:98` | CoreRequest variant count stated as "~100" — actual is **166** variants | `core.rs:1132` — `pub enum CoreRequest` with 166 variants | batch5_core_transport |
| H2 | `protocol.md:178` | CoreResponse variant count stated as "~60" — actual is **110** variants | `core.rs:542` — `pub enum CoreResponse` with 110 variants | batch5_core_transport |
| H3 | `protocol.md:235` | CoreEvent variant count stated as "~40" — actual is **76** variants | `core.rs:1953` — `pub enum CoreEvent` with 76 variants | batch5_core_transport |
| H4 | `session.md:53` | CREATE TABLE count stated as "52+" — actual is **71** | `session/schema.rs` — 71 `CREATE TABLE` statements | batch4_git_persistence |
| H5 | `session.md:109` | SESSION_COLUMNS stated as "22 columns" — actual is **27** | Constant value in session state module | batch4_git_persistence |
| H6 | `jobs.md:126` | JobStore method count stated as "16 methods" — actual is **21** (missing `set_job_labels`, `create_job_with_labels`, `get_jobs`, `count_jobs_by_kind_state`, `list_job_records`) | `mod.rs:1274` — 21 trait methods | batch6_workspace_jobs |
| H7 | `jobs.md:220` | `recover_at_startup` line reference `scheduler.rs:1281` — actual is `scheduler.rs:1529` (off by 248 lines) | `scheduler.rs:1529` | batch6_workspace_jobs |
| H8 | `scheduler.md:57` | Main loop line reference `scheduler.rs:625` — actual is `scheduler.rs:809` (off by 184) | `scheduler.rs:809` | batch6_workspace_jobs |
| H9 | `scheduler.md:65` | Reconciliation line reference `scheduler.rs:435` — actual is `scheduler.rs:583` (off by 148) | `scheduler.rs:583` | batch6_workspace_jobs |
| H10 | `scheduler.md:73` | Admission line reference `scheduler.rs:680` — actual is `scheduler.rs:833` (off by 153) | `scheduler.rs:833` | batch6_workspace_jobs |
| H11 | `scheduler.md:129` | AdmissionController line reference `admission.rs:27` — actual is `admission.rs:99` (line 27 is `AdmissionDecision` enum) | `admission.rs:99` | batch6_workspace_jobs |
| H12 | `scheduler.md:357` | Shutdown line reference `scheduler.rs:1102` — actual is `scheduler.rs:1317` (off by 215) | `scheduler.rs:1317` | batch6_workspace_jobs |
| H13 | `codegg_core.md:20–47` | Module table lists 27 modules — actual `lib.rs` declares **40** public modules | `crates/codegg-core/src/lib.rs` — 40 `pub mod` declarations | batch7_provider_config |
| H14 | `native_crates.md:22,153` | Same "40 modules" claim in codegg_core.md — actual is 42 (both docs stale) | `lib.rs` has 42 `pub mod` | batch7_provider_config |
| H15 | `config.md:774` | `ProviderConfig::merge()` referenced at `schema.rs:774` — that line is `ServerConfig::merge()`; ProviderConfig's merge is at `schema.rs:827` | `schema.rs:827` | batch7_provider_config |

## HIGH Severity — Code Issues

| # | Module | Issue | Location |
|---|--------|-------|----------|
| — | — | No security-critical or behavioral code bugs found across all batches | — |

## HIGH Severity — Stale Content

| # | File | Content | Reason |
|---|------|---------|--------|
| HS1 | `git.md:204` | `render_argv.rs` file reference — actual file is `render.rs` | File was renamed during refactoring |
| HS2 | `git.md:231` | `GitMutationExecutor` at `git_mutations.rs:703` — actual is `:471` | Definition moved during refactoring |
| HS3 | `git.md:233–235` | `RepoSnapshot` at `git_mutations.rs:246`, `StateDelta` at `:278`, `MutationOutcome` at `:324` — all three are defined in `codegg-git/src/workflow.rs` | Types re-exported through git_mutations but defined in workflow.rs |

## MEDIUM Severity — Documentation Fixes

| # | File | Issue | Evidence |
|---|------|-------|----------|
| M1 | `projection.md:138,306` | ProjectionEvent variant count "39" — actual is **46** (missing `ConvergenceUpserted`, 6 `ToolProgram*` variants) | `event.rs:128` — 46 variants |
| M2 | `protocol.md:237` | CoreEvent Snapshot group stated as "(5)" — actual is **6** variants | `core.rs` — 6 snapshot variants |
| M3 | `protocol.md:83` | EventEnvelope line reference `core.rs:125` — actual is `core.rs:531` (off by 406) | `core.rs:531` |
| M4 | `protocol.md:96` | CoreRequest line reference `core.rs:524` — actual is `core.rs:1132` (off by 608) | `core.rs:1132` |
| M5 | `protocol.md:176` | CoreResponse line reference `core.rs:137` — actual is `core.rs:542` (off by 405) | `core.rs:542` |
| M6 | `protocol.md:233` | CoreEvent line reference `core.rs:1058` — actual is `core.rs:1953` (off by 895) | `core.rs:1953` |
| M7 | `run_store.md` | `FsRunStore` at `:1110` — actual `:1126`; `MemRunStore` at `:1730` — actual `:1752`; all view models (`:861–:930`) off by 15–22 lines | Systematic drift in run_store.rs |
| M8 | `acp.md` | All function line references (ensure_client, absolute_cwd, prompt_text, etc.) consistently off by **13 lines** | Code shifted down by new imports |
| M9 | `provider.md:770` | `register_builtin_with_config()` at line 770 — actual is line 844 (off by 74) | `provider_core.rs:844` |
| M10 | `resilience.md` | `is_available()` at `circuit.rs:80` — deprecated in source; doc does not mention deprecation | `circuit.rs:80` — `#[deprecated]` annotation |
| M11 | `resilience.md:156,181` | `record_success` at `:156` — actual `:194`; `record_failure` at `:181` — actual `:219` (off by 38 each) | `circuit.rs:194,219` |
| M12 | `config.md:203,284,686` | `Config` struct at `schema.rs:203` — actual `:217`; `ProviderConnectionsConfig` at `:284` — actual `:339`; `ServerConfig` at `:686` — actual `:741` (off by 14–55) | `schema.rs` |
| M13 | `workspace.md:87` | Schema migration v22 at `schema.rs:963` — actual `migrate_v22` at `schema.rs:1064` (off by ~101) | `schema.rs:1064` |
| M14 | `permission.md:278` | debug mode restricted_tools summary table shows only 3 tools but actual `modes.rs:183` restricts more | Summary is incomplete |
| M15 | `overview.md:257` | Crypto module mapped to `auth/` directory — source is `crates/codegg-providers/src/crypto.rs` | Wrong source path |
| M16 | `identity.md:17–19` | Lists 10 identity newtypes but source defines **13** (missing `AgentRunGroupId`, `AgentRunMessageId`, `ChatMessageId`) | `identity.rs:251,274,280` |
| M17 | `git.md:31` | Mutation action count "40" — actual is **42** | `GitTool` schema has 42 mutation entries |
| M18 | `test_runner.md:334–335` | `DelegatedTestRun` at `runner.rs:260` — actual `:356`; `setsid` at `runner.rs:290` — actual `:386`; kill at `:478` — actual `:595` | Multiple line refs stale by 96–117 lines |
| M19 | `bus.md:18` | AppEvent "Other" category table lists 8 variants but actually contains 10 | `events.rs:61` |
| M20 | `command_intent.md:240,269` | `CommandIntentFamily` at `schema.rs:2904` — actual `:3005`; `CommandIntentConfig` at `schema.rs:2741` — actual `~:2900` | `schema.rs` |
| M21 | `provider.md:136–142` | ProviderCapabilities listed as ~3 fields — actual struct has **14 fields** | `provider_core.rs` — 14-field struct |
| M22 | `tts.md:22–24` | `Tts` struct described as `speaking: Mutex<AtomicBool>` — actual field is `speaking: AtomicBool` (no Mutex) | `AtomicBool` sufficient since methods take `&self` |

## MEDIUM Severity — Stale Content

| # | File | Content | Reason |
|---|------|---------|--------|
| MS1 | `overview.md:196` | "state management across 6 domains" | Expanded to 22 state modules |
| MS2 | `tui.md:361–362` | "All dispatch arms are `fn` (non-async). No `.await` points." | `dispatch_tui_command` is `async fn` with 5 `.await` calls |
| MS3 | `agent.md:252` | `AgentLoop` at `loop.rs:403` — actual `:86` (off by 317 lines) | Refactoring moved struct definition |
| MS4 | `agent.md:254` | "32 direct fields" — actual AgentLoop has **30** fields | Fields removed during refactoring |
| MS5 | `cache-aware-context.md:286` | `ContextPolicyConfig` said to be in `src/context/policy.rs` — actually in `codegg_config::schema` crate | Config struct lives in config crate |
| MS6 | `compaction.md:21` | `compact_if_needed` at `loop.rs:1838` — actual is `context_runtime.rs:620` (off by ~1200 lines, function moved) | M004 refactoring moved function |
| MS7 | `native_crates.md:154` | codegg-core "27 modules" — actual is **40** | `lib.rs` has 40 `pub mod` |
| MS8 | `overview.md:178` | "66 files" in `src/tool/` — actual is 68 `.rs` files | Minor file count drift |
| MS9 | `run_store.md:272` | "19 unit tests" — actual is **18** | Test removed or merged |
| MS10 | `worktree.md:208` | "integration tests (11 tests)" — actual is **14** | Tests added since doc was written |
| MS11 | `git_phase_f_handoff.md:116` | "402 egggit tests" — actual is 75 egggit + 358 codegg-git = 433 total | Conflation of crate counts |
| MS12 | `git_polish_verification_handoff.md:34` | `operation.rs` has "47 variants" — actual is **54** | Growth since Phase F closure |
| MS13 | `git.md:272` | `process_policy.rs` described as "canonical source of truth" — it's a re-export shim; canonical is `egggit::process` | Misidentified canonical source |

## LOW Severity — Documentation Fixes

| # | File | Issue |
|---|------|-------|
| L1 | `goal.md:203–215` | 8 of 12 GoalStore line references stale by 3–90 lines |
| L2 | `agent.md:214,239,298,386` | Multiple `mod.rs` line refs should point to `definition.rs` |
| L3 | `agent-tool-surface.md:18` | `apply_tool_exposure_filter()` in `loop.rs` — actual implementation in `request_preparation.rs:15` |
| L4 | `worktree.md:142–170` | Multiple line refs off by 1–15 lines (Worktree, WorktreeInfo, create_worktree, etc.) |
| L5 | `overview.md:149` | Command Routing listed as "pipeline stage 3" but routing doc identifies it as a compatibility facade |
| L6 | `exec.md:179` | `classify_error()` at lines 204–272 — actual starts at line 217 |
| L7 | `test_runner.md:286` | Two `DEFAULT_MAX_REPORT_BYTES` constants exist in both `runner.rs:26` and `report.rs` — clarify canonical |
| L8 | `human_shell.md:128` | `ShellOutputStore` defaults "1 MB/cmd (head 256KB + tail 256KB)" — math sums to 512KB, not 1 MB |
| L9 | `bus.md:164,167` | AppEvent at `events.rs:60` — actual `:61`; PermissionDecision at `mod.rs:11` — actual `:12` |
| L10 | `projection.md:301–308` | Multiple line refs off by 1–3 lines (caps.rs, snapshot.rs, event.rs) |
| L11 | `memory.md:72–113` | MemoryStore method line numbers consistently off by +2 |
| L12 | `memory.md:115,127` | `PatternDetector` at `:40` — actual `:76` (off by 36); `ScoredMemory` at `:269` — actual `:306` (off by 37) |
| L13 | `tui.md:366,611,646` | `App` at `:865` — actual `:222`; `Component` at `:110` — actual `:165`; `TuiMsg` at `:86` — actual `:97` |
| L14 | `tui.md:14` | `app/mod.rs` "~15,340 lines" — actual 13,675 |
| L15 | `tts.md:16` | TUI integration at `app/mod.rs:9820–9921` — actual `:7583–7640` (off by ~2240) |
| L16 | `upgrade.md:14,50,80,81` | Line refs off by 1–12 lines |
| L17 | `util.md:80` | `tool_interner()` at `tool/mod.rs:711` — actual defined at `util/interner.rs:41` |
| L18 | `scheduler.md:87` | `JobScheduler` at `scheduler.rs:99` — actual `:135` (off by 36) |
| L19 | `scheduler.md:112` | `JobSubmissionService` at `submission.rs:83` — actual `:86` (off by 3) |
| L20 | `tui.md:931` | "97 render regression tests" — actual is 99 |
| L21 | `testing.md:207–215` | CI Structure lists 7 steps — actual CI has 8 (missing TUI project authority guard) |
| L22 | `server.md:28` | `run_server` at `http.rs:170` — actual `:182` |
| L23 | `provider.md:50–432` | Multiple line refs off by 1–11 lines throughout document |
| L24 | `overview.md:189` | "Tools and Capabilities" table omits `context_read` tool |
| L25 | `worktree.md:163–166` | `list_worktrees` at `:33` — actual `:31`; `create_worktree` at `:40` — actual `:38`; `remove_worktree` at `:68` — actual `:83` |
| L26 | `run_store.md:87` | `ActualBackend` "(9)" variants — actual is 8 (no `Unrouted` variant) |
| L27 | `tts.md:55` | `init()` return type missing — actual returns `Result<(), AppError>` |
| L28 | `git.md:236` | `MutationResult` at `git_mutations.rs:352` — actual in `codegg-git/src/workflow.rs:86` |

## LOW Severity — Stale Content

| # | File | Content | Reason |
|---|------|---------|--------|
| LS1 | `session.md:56–98` | Table groupings omit `agent_run_group`, `agent_run_group_member`, `agent_run_journal`, `agent_run_mailbox`, `agent_run_result`, `managed_worktree`, `worktree_lease` tables | Added in undocumented migrations (v38–v50) |
| LS2 | `storage.md:238–261` | Migration list omits v38–v45 and v50 | Selective coverage without catch-all |
| LS3 | `mcp.md` | `protocol.rs` not listed in file layout table | Internal `pub(crate)` file omitted |
| LS4 | `git_phase_f_handoff.md:111` | `--all-features` test command | Contradicts AGENTS.md guidance |
| LS5 | `git.md:333–344` | Duplicate numbering (two items numbered "12") | Renumbering needed |

## Improvements — Cross-Cutting

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| I1 | protocol.md | Regenerate CoreRequest/CoreResponse/CoreEvent variant tables from source enums; add CI assertion test for variant counts | Prevents recurring 40–90% undercount drift |
| I2 | projection.md | Regenerate ProjectionEvent table from `event.rs:128` | Prevents variant count drift |
| I3 | jobs.md, scheduler.md | Replace hardcoded line numbers with `fn name` anchors or "last verified" timestamps — both files are 1700+ lines and shift with every edit | Eliminates recurring stale-line maintenance |
| I4 | acp.md | Update all 8 function line references (uniformly off by 13 lines) | Single-pass fix for systematic drift |
| I5 | provider.md | Document full ProviderCapabilities field set (14 fields) rather than summarizing 3 | Reduces future drift; becomes authoritative for integration |
| I6 | codegg_core.md | Add a count-of-modules assertion test in CI (like `built_in_command_count_matches_release_docs`) | Prevents 27-vs-40 module count drift |
| I7 | run_store.md | Bulk refresh line numbers for view models (`:861–:930` → `:877–:947`) | Systematic 15–22 line drift |
| I8 | resilience.md | Document `call()` as preferred admission path replacing deprecated `is_available()` | Guides future callers to atomic API |
| I9 | identity.md | Update newtypes list from 10 to 13; add "Evolution" note for `typed_identity!` macro | Completeness |
| I10 | overview.md | Add `ide` module to module map; add `context_read` to tools table; update TUI "6 domains" → "22 state modules" | Meta-document accuracy |

## Cross-Module Issues

### 1. Variant Count Drift Pattern
Multiple protocol-layer enums have grown significantly without documentation updates:
- `CoreRequest`: 100 → 166 (+66%)
- `CoreResponse`: 60 → 110 (+83%)
- `CoreEvent`: 40 → 76 (+90%)
- `ProjectionEvent`: 39 → 46 (+18%)
- `AppEvent`: 45 → 53 (+18%)

**Recommendation**: Add CI assertion tests that assert documented variant counts match actual enum variant counts (pattern exists for commands: `assert_eq!(CommandRegistry::built_in_commands().len(), 139)`).

### 2. Systematic Line-Number Drift
Several modules have line references that are uniformly stale, indicating they were captured at a specific commit and never refreshed:
- `acp.md`: All 8 function refs off by exactly 13 lines
- `run_store.md`: View model refs off by 15–22 lines
- `provider.md`: Line refs off by 10–74 lines throughout
- `goal.md`: 8 of 12 GoalStore refs stale by 3–90 lines
- `scheduler.md`: 6 refs off by 36–248 lines

**Recommendation**: For high-churn files (>500 lines), prefer file-path-only references or anchor-based linking. Add a `last_verified` timestamp field to docs referencing specific line numbers.

### 3. Module Count Inconsistency
`codegg_core.md` and `native_crates.md` both claim 27 modules for `codegg-core`, but `lib.rs` declares **40** (or **42** per batch7's recount). The discrepancy suggests two rounds of module additions were not propagated to docs.

**Recommendation**: Add a CI guard similar to `check_project_catalog_invariants.py` that asserts the documented module count matches `lib.rs` `pub mod` count.

### 4. File-Path Staleness from Refactoring
Several functions were moved between modules during refactoring (M002–M004) but docs still reference old locations:
- `compact_if_needed`: `loop.rs` → `context_runtime.rs` (off by ~1200 lines)
- `apply_tool_exposure_filter`: `loop.rs` → `request_preparation.rs`
- `Agent` struct: `mod.rs` → `definition.rs`
- `GitMutationExecutor`, `RepoSnapshot`, `StateDelta`: `git_mutations.rs` → `codegg-git/src/workflow.rs`
- `render_argv.rs` → `render.rs`
- `check_kill_switches`: `tool/bash.rs` → `tool/bash/policy.rs`

**Recommendation**: When moving public items, add a `// Moved from <old_path> in <commit>` comment at the new location, or update the architecture doc in the same PR.

### 5. Historical Snapshot Docs
Handoff documents (`git_phase_f_handoff.md`, `git_polish_verification_handoff.md`) contain snapshot-time counts that are naturally stale:
- egggit tests: 71 → 75
- codegg-git tests: 331 → 358
- operation.rs variants: 47 → 54
- "402 egggit tests" claim appears to be a conflation (actual: 75 + 358 = 433 total)

These are acceptable as historical records but should carry "last verified" timestamps to prevent confusion.

## Verified Counts (Cross-Check vs overview.md)

| Claim | Doc Value | Actual | Match |
|-------|-----------|--------|-------|
| Tool registration statements | 53 | 53 | ✅ |
| LSP servers | 39 | 39 | ✅ |
| AppEvent variants | 53 | 53 | ✅ |
| Built-in slash commands | 139 | 139 | ✅ |
| Built-in agents | 10 | 10 | ✅ |
| DB tables | 71 | 71 | ✅ |
| Storage layout version | 56 | 56 | ✅ |
| Integration test files | 189 | 189 | ✅ |
| Architecture docs | 77 | 77 | ✅ |
| CI guard scripts | 21 | 21 | ✅ |
| Env-var providers | 15 | 15 | ✅ |
| GitOperation variants | 54 | 54 | ✅ |
| GitRiskClass variants | 11 | 11 | ✅ |
| Bundled themes | 50 | 50 | ✅ |
| CoreRequest variants | ~100 | **166** | ❌ STALE |
| CoreResponse variants | ~60 | **110** | ❌ STALE |
| CoreEvent variants | ~40 | **76** | ❌ STALE |
| ProjectionEvent variants | 39 | **46** | ❌ STALE |
| codegg-core modules | 27 | **40** | ❌ STALE |
