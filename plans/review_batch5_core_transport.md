# Review: batch5 core-transport

**Reviewed**: 2026-09-11
**Files**: architecture/core.md, architecture/server.md, architecture/client.md, architecture/acp.md, architecture/protocol.md, architecture/projection.md, architecture/bus.md

## Summary

All 7 documents are structurally sound and accurately describe the module boundaries, transport modes, and protocol design. The most pervasive issue is **stale variant counts**: protocol.md understates CoreRequest (~100 → actual 166), CoreResponse (~60 → actual 110), CoreEvent (~40 → actual 76); projection.md understates ProjectionEvent (39 → actual 46); bus.md understates AppEvent (45 → actual 53). Additionally, protocol.md has severely stale line-number references for CoreRequest/CoreResponse/CoreEvent/EventEnvelope (hundreds of lines off), and projection.md has stale line references for ProjectionClientController and all three ToolProgram DTOs. ACP line references are consistently off by ~13 lines (functions shifted down by new code). daemon.rs test reference in core.md is off by 1737 lines.

## Documentation Issues

| # | File | Line | Issue | Suggested fix |
|---|------|------|-------|---------------|
| 1 | bus.md | 18,43 | **AppEvent variant count**: Doc says 45 variants; actual code has 53 variants (`events.rs:61`). Missing categories: `AgentRunUpdated/Progress/Terminal/ControlUpdated`, `WorktreeUpdated`, `AgentRunGroupUpdated`, `ConvergenceUpdated`, `RunRerunLinked`. | Update count to 53 and add missing variant rows to the table. |
| 2 | protocol.md | 98 | **CoreRequest variant count**: Doc says "~100 variants"; actual is 166 variants (`core.rs:1132`). The ~100 figure is stale by ~66 variants (40% understatement). | Update to "~166 variants" and reconcile the grouped sub-counts. |
| 3 | protocol.md | 178 | **CoreResponse variant count**: Doc says "~60 variants"; actual is 110 variants (`core.rs:542`). Missing many response families (InteractiveProcess*, Presence*, Chat*, EditCheckpoint*, Audit*, ToolProgram responses, etc.). | Update to "~110 variants" and list missing families. |
| 4 | protocol.md | 235 | **CoreEvent variant count**: Doc says "~40 variants"; actual is 76 variants (`core.rs:1953`). Missing families: Job (14+), Schedule (6), Run (9), ToolProgram (3), InteractiveProcess, Presence, Chat (5). | Update to "~76 variants" and add missing families. |
| 5 | protocol.md | 237 | **CoreEvent Snapshot group**: Doc says "Snapshot (5)" but lists 6 variants: `SnapshotSession`, `SnapshotWorkspace`, `SnapshotModels`, `AssetRefreshCompleted`, `ConnectionRotated`, `ConnectionStateChanged`. | Change count from 5 to 6. |
| 6 | protocol.md | 83 | **EventEnvelope line reference**: Doc says `core.rs:125`; actual is `core.rs:531`. Off by 406 lines. | Update to `core.rs:531`. |
| 7 | protocol.md | 96 | **CoreRequest line reference**: Doc says `core.rs:524`; actual is `core.rs:1132`. Off by 608 lines. | Update to `core.rs:1132`. |
| 8 | protocol.md | 176 | **CoreResponse line reference**: Doc says `core.rs:137`; actual is `core.rs:542`. Off by 405 lines. | Update to `core.rs:542`. |
| 9 | protocol.md | 233 | **CoreEvent line reference**: Doc says `core.rs:1058`; actual is `core.rs:1953`. Off by 895 lines. | Update to `core.rs:1953`. |
| 10 | projection.md | 138,306 | **ProjectionEvent variant count**: Doc says 39 variants (both in text and table); actual is 46 variants (`event.rs:128`). Missing: `ConvergenceUpserted`, `ToolProgramAdmitted`, `ToolProgramStarted`, `ToolProgramProgress`, `ToolProgramWaitingForCall`, `ToolProgramWaitingForJob`, `ToolProgramRetryBackoff`. | Update count to 46 and add missing variant rows. |
| 11 | projection.md | 309 | **ProjectionClientController line reference**: Doc says `controller.rs:82`; actual is `controller.rs:219`. Off by 137 lines. | Update to `controller.rs:219`. |
| 12 | projection.md | 315 | **ToolProgramSummary line reference**: Doc says `dto.rs:583`; actual is `dto.rs:822`. Off by 239 lines. | Update to `dto.rs:822`. |
| 13 | projection.md | 316 | **ToolProgramDetail line reference**: Doc says `dto.rs:708`; actual is `dto.rs:947`. Off by 239 lines. | Update to `dto.rs:947`. |
| 14 | projection.md | 317 | **ToolProgramCallPage line reference**: Doc says `dto.rs:679`; actual is `dto.rs:918`. Off by 239 lines. | Update to `dto.rs:918`. |
| 15 | core.md | 552 | **Test line reference**: `turn_submit_uses_injected_runtime` referenced at `daemon.rs:6226`; actual is `daemon.rs:4489`. Off by 1737 lines. | Update to `daemon.rs:4489`. |
| 16 | server.md | 28 | **run_server line reference**: Doc says `http.rs:170`; actual is `http.rs:182`. Off by 12 lines. | Update to `http.rs:182`. |
| 17 | acp.md | 17 | **File size claim**: Doc says "~720 lines"; actual file is 736 lines. | Update to "~736 lines". |
| 18 | acp.md | 110 | **ensure_client line reference**: Doc says `src/acp.rs:288`; actual definition is `src/acp.rs:301`. Off by 13 lines. | Update to `src/acp.rs:301`. |
| 19 | acp.md | 111 | **absolute_cwd line reference**: Doc says `src/acp.rs:304`; actual is `src/acp.rs:317`. Off by 13 lines. | Update to `src/acp.rs:317`. |
| 20 | acp.md | 112 | **prompt_text line reference**: Doc says `src/acp.rs:330`; actual is `src/acp.rs:343`. Off by 13 lines. | Update to `src/acp.rs:343`. |
| 21 | acp.md | 113 | **native_agents line reference**: Doc says `src/acp.rs:355`; actual is `src/acp.rs:368`. Off by 13 lines. | Update to `src/acp.rs:368`. |
| 22 | acp.md | 115 | **replay_snapshot line reference**: Doc says `src/acp.rs:412`; actual is `src/acp.rs:425`. Off by 13 lines. | Update to `src/acp.rs:425`. |
| 23 | acp.md | 116 | **handle_event line reference**: Doc says `src/acp.rs:454`; actual is `src/acp.rs:467`. Off by 13 lines. | Update to `src/acp.rs:467`. |
| 24 | acp.md | 117 | **event_is_terminal line reference**: Doc says `src/acp.rs:489`; actual is `src/acp.rs:502`. Off by 13 lines. | Update to `src/acp.rs:502`. |
| 25 | acp.md | 118 | **cancel_if_ready line reference**: Doc says `src/acp.rs:268`; actual function definition is `src/acp.rs:281`. Off by 13 lines. (Line 268 is a call site, not the definition.) | Update to `src/acp.rs:281`. |
| 26 | protocol.md | 272 | **TuiMessage variant grouping**: Doc lists "Client-to-Server (3)" then "Connection (3)" then "Response (2)" — but `EventEnvelope` is listed as a "Client-to-Server" variant when it's actually a bidirectional wrapper. `ProjectionCompatibilityDiagnostic` is not listed in the Projection group count (19 actually, not 17). | Clarify `EventEnvelope` is a bidirectional wrapper. Update Projection count to 19. |
| 27 | projection.md | 301 | **ProjectionCapabilities line reference**: Doc says `caps.rs:39`; actual is `caps.rs:40`. Off by 1 line. | Update to `caps.rs:40`. |
| 28 | projection.md | 302 | **SessionProjectionSnapshot line reference**: Doc says `snapshot.rs:26`; actual is `snapshot.rs:29`. Off by 3 lines. | Update to `snapshot.rs:29`. |
| 29 | projection.md | 307 | **ProjectionEvent line reference**: Doc says `event.rs:127`; actual is `event.rs:128`. Off by 1 line. | Update to `event.rs:128`. |
| 30 | projection.md | 308 | **ProjectionEnvelope line reference**: Doc says `event.rs:53`; actual is `event.rs:54`. Off by 1 line. | Update to `event.rs:54`. |
| 31 | bus.md | 164 | **AppEvent line reference**: Doc says `bus/events.rs:60`; actual is `bus/events.rs:61`. Off by 1 line. | Update to `bus/events.rs:61`. |
| 32 | bus.md | 167 | **PermissionDecision line reference**: Doc says `bus/mod.rs:11`; actual is `bus/mod.rs:12`. Off by 1 line. | Update to `bus/mod.rs:12`. |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | `codegg-core` bus | `AppEvent` has grown from 45 to 53 variants without documentation update. The "Other" group in bus.md lists 9 variants but only names 8 (`ContextUpdated` and `PluginUiEffect` are included — total is actually 10 variants in "Other" including `ContextUpdated`). | `crates/codegg-core/src/bus/events.rs:61` | LOW — doc lag only |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | bus.md | The "Server Route Limitation" section (lines 149-157) notes that `/api/permission` and `/api/question` SSE routes return empty lists because they can't filter by session. Now that scoped `get_pending_for_session()` exists, this section should note that the routes should migrate to use scoped queries. | Prevents future confusion about whether session-scoped queries are available. |
| 2 | protocol.md | The `CoreRequest` grouped counts (Asset Refresh 3, Connection Lifecycle ~20, Session Lifecycle 19, etc.) are stale and undercount many groups. A structured audit of each group against the actual enum would prevent drift. Consider adding a test that asserts the doc-listed variant counts match reality. | Prevents recurring count drift. |
| 3 | projection.md | `ProjectionEvent` has grown from 39 to 46 variants. The doc table should be regenerated from the actual enum to catch future additions. | Keeps projection documentation accurate. |
| 4 | core.md | The test line reference `daemon.rs:6226` for `turn_submit_uses_injected_runtime` is off by 1737 lines. Consider referencing test names rather than line numbers, which are inherently fragile. | Reduces maintenance burden. |
| 5 | acp.md | All function line references (ensure_client, absolute_cwd, prompt_text, etc.) are off by exactly 13 lines, suggesting they were captured at a specific commit and never updated. A CI guard or test that validates line references would prevent drift. | Prevents systematic drift across the ACP doc. |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | protocol.md:98 | `CoreRequest` described as "~100 variants" | Actual is 166; count is off by 66%. |
| 2 | protocol.md:178 | `CoreResponse` described as "~60 variants" | Actual is 110; count is off by 83%. |
| 3 | protocol.md:235 | `CoreEvent` described as "~40 variants" | Actual is 76; count is off by 90%. |
| 4 | protocol.md:237 | CoreEvent Snapshot group "(5)" | Actually 6 variants. |
| 5 | projection.md:138,306 | `ProjectionEvent` described as "39 variants" | Actual is 46 variants; off by 18%. |
| 6 | acp.md:17 | File described as "~720 lines" | Actual is 736 lines. |
