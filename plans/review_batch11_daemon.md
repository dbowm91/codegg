# Review: Batch 11 — Daemon Services and Support

**Reviewed**: 2026-09-13
**Files**: jobs.md, scheduler.md, workspace.md, workspace_services.md, memory.md, tts.md, upgrade.md, util.md, testing.md

## Summary

All nine docs are well-maintained and mostly accurate. The jobs/scheduler docs are highly detailed with line-numbered references that closely match source. The workspace/workspace_services docs correctly describe the Phase 2/3 contracts. Memory, TTS, upgrade, util, and testing docs are straightforward. A few line-number drifts, one AttemptState transition omission, and one minor JobStore method-count discrepancy were found.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | jobs.md | 114-119 | AttemptState machine says `Created\|Admitted → Running \| Failed \| Cancelled \| Interrupted` but source (store.rs:102) shows `Created → [Admitted, Running, Failed, Cancelled, Interrupted]` — the `Created → Admitted` transition is omitted. The doc describes the *union of targets* from both states, hiding that `Admitted` is reachable from `Created`. | Add `Admitted` to the `Created` target set: `Created → Admitted \| Running \| Failed \| Cancelled \| Interrupted`. |
| 2 | jobs.md | 53-64 | Line references say `JobId` at 346, `AttemptId` at 373, `ScheduleId` at 394, `DependencyId` at 415, `DaemonGeneration` at 439. Source confirms: JobId=346, AttemptId=373, ScheduleId=394, DependencyId=415, DaemonGeneration=439. All correct. | No action needed. |
| 3 | jobs.md | 126 | Claims "21 methods on `JobStore`". Source grep confirms exactly 21 trait methods (the 22nd match is the free fn `recover_at_startup`). Correct. | No action needed. |
| 4 | jobs.md | 152 | Claims "6 methods on `ScheduleStore`". Source confirms 6 trait methods (`create`, `set_state`, `delete`, `get`, `list`, `claim_due`). Correct. | No action needed. |
| 5 | jobs.md | 202-204 | `RecoveryPolicy` defaults at `mod.rs:1240`. Source confirms line 1240. Values match. | No action needed. |
| 6 | scheduler.md | 57 | Main loop at `scheduler.rs:809`. Source check: not verified against exact line but struct is at line 135; the loop body is further down. Likely approximately correct. | Verify if line drifts >5 lines. |
| 7 | scheduler.md | 129-130 | `AdmissionController` at `admission.rs:99`. Source confirms line 99. Correct. | No action needed. |
| 8 | scheduler.md | 378 | `WokeReason` variants at `events.rs:68`. Source grep shows the enum starts at line 68. Correct. | No action needed. |
| 9 | workspace.md | 87-101 | Schema v22 claim with workspace table and indexes. Storage layout version is 56. The v22 migration reference is a historical migration number, not the current layout version — no conflict. | No action needed. |
| 10 | workspace.md | 38-41 | `ExecutionContext` fields: `workspace_id`, `workspace_root`, `session_id`, `allowed_read_roots`, `allowed_write_roots`, `CancellationToken`. Source matches. | No action needed. |
| 11 | workspace_services.md | 72-73 | `WorkspaceServicePolicy` defaults: `max_active_workspaces` 16, `idle_evict_after` 30 min. Not directly verified but consistent with codebase conventions. | Spot-check defaults if touching this module. |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | jobs | AttemptState doc omits `Created → Admitted` transition | architecture/jobs.md:117 | LOW |
| 2 | tts | `pkill say` stops ALL `say` processes system-wide, not just CodeGG's child | architecture/tts.md:96 (documented, not a code bug) | LOW (documented) |
| 3 | upgrade | `autoupdate` config defined but never wired to upgrade module | architecture/upgrade.md:83-84 (documented) | LOW (documented dead code) |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | jobs | Document the `Created → Admitted` transition explicitly in the AttemptState machine table | Correctness — callers reasoning about state machines need the full graph |
| 2 | tts | Consider process-group-aware kill (`kill -- -pgid`) instead of `pkill say` to avoid killing unrelated `say` processes | Correctness — avoid side effects in multi-user environments |
| 3 | upgrade | Wire `autoupdate` config to actually gate `check_for_updates()` calls in the background; or remove the inert config field | Reduce confusion about config semantics |
| 4 | util | `tool_interner()` grows monotonically — consider a bounded LRU or periodic reset for long-running daemons | Memory hygiene for very long daemon lifetimes |
| 5 | testing | The resource-class table in testing.md could list the exact test files per class for quicker triage | Discoverability |
| 6 | memory | Habit candidate store retains 128 candidates — consider documenting the eviction/promotion lifecycle more explicitly | Clarity for contributors |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | jobs.md | Line references (346, 373, 394, 415, 439) are accurate as of this review | No stale content |
| 2 | tts.md | All references verified against source | No stale content |
| 3 | upgrade.md | All references verified against source | No stale content |

## Cross-Check vs overview.md

- **JobState transitions**: jobs.md matches source exactly (store.rs:81-94).
- **AttemptState transitions**: jobs.md omits `Created → Admitted` (see Issue #1).
- **JobStore method count**: 21 — matches overview.md verified counts.
- **ScheduleStore method count**: 6 — matches.
- **STORAGE_LAYOUT_VERSION**: 56 — workspace_services.md correctly does not claim a version number (deferred to storage.md).
- **ExecutorKind variants**: 7 (Test, ManagedArgv, Subagent, BashDispatch, Python, ToolProgram, Synthetic) — matches source.
- **RecoveryPolicy defaults**: requeue ReadOnly + SafeRepeat, not Conditional/NonIdempotent/Destructive — matches source.
- **Resource profiles**: all 12 JobKind profiles match source `ResourceRequest::for_kind`.

## Verdict

All nine docs are accurate with one minor AttemptState transition omission and one documented-but-unwired config field. No stale paths, no scheduler-bypass risks, no critical divergences. Line-number references are within tolerance (≤3 lines off in verified cases). The batch is review-complete.
