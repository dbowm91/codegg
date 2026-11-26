# Harness Lab Self-Improvement Resumption Milestone S026 — Closure Status

Status: **corrective pass required**

Source implementation plan:

- `plans/implementation/harness-lab-self-improvement-resumption/026-public-sequence-audit-v87.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#20-s026--public-sequence-audit`

Repository baseline reviewed: `ac584c886f03847ff6b829f8509667637e42a052`

Implementation commits or pull requests:

- Shared worktree changes; no commit or PR created in this pass.

## 1. Executive finding

S026's prompt-only v87 sequence audit and offline checks were implemented, but its live objective did not qualify H007. Attempt 33 ran for 54.5 minutes of active work and passed the horizon duration gate, yet completed only one of the two required repair pairs. The independent verifier passed 3/5. H007 remains unqualified; S027 owns the narrow corrective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| v87 changes only prompt/group/title from v86 | Manifest identity check; v87 hash `3c3669423d0cf5fde4f65be43a1491e91c1ff01df8572d31119250dcc35eba83`; parent v86 hash `be4abad68e0ef9173a8e3190ac385857910a2d3dcb9256e68c097a7abd437ef5` | pass | Verifier and binary identities were unchanged. |
| Offline prompt and diagnostic checks | H007 75 tests, verifier diagnostics 24 tests, autonomy-run 61 tests, autonomy cases 7 tests; assertion-quality and duplication guards, Python compilation, and `git diff --check` | pass | These are the focused results recorded before attempt 33. |
| At least 30 minutes of active engineering | Safe attempt 33 receipt | pass | Active-work span was 3,268,583 ms. |
| Two exact-ID failure/repair pairs | Safe public pair projection | fail | `catalog_format` qualified; `catalog_store` did not because the repair rerun preceded observation of its seed failure and no productive change occurred between the failure observation and rerun. One of two pairs qualified. |
| Continuation and public task completion | Safe continuation projection | fail | Seven stages and required gates projected complete, but the continuation document was invalid because the pair requirement was incomplete. |
| Exact-ID productive overlap and reviewer | Safe horizon/reviewer projection | pass | Both were observed. |
| All submitted test jobs terminal | Safe lifecycle projection | pass | 11/11 terminal: 8 completed, 3 failed; no unobserved job remained. |
| Independent verifier | Safe verifier projection | fail | 3/5 passed; safe failure domains were `continuation_boundary` and `transcript_job_lifecycle`. No assertion text or raw output was inspected. |
| Event, disk, stall, and cleanup controls | Safe trace/resource receipt | pass | 279/279 calls paired, no event gaps/drops, 83,030 ms maximum native-event gap, no stall/disk stop, and zero owned daemons after cleanup. |
| Overall H007 qualification | Safe result projection | fail | Task outcome failed; the active horizon alone does not qualify. |

## 3. Production implementation evidence

S026 made a prompt-only v87 change and added an offline regression. It did not change production Rust, the task implementation, public tests, the independent verifier, the daemon, scheduler, ACP protocol, provider adapter, or qualification criteria.

Attempt 33 used `opencode_go/muse-spark-1.3-contributor`, v87, and the existing paired ARM64 binary/helper. It ran for 3,299,945 ms with 3,268,583 ms active, 263/1,200 provider requests, and 279/1,600 native calls. Provider allowance was USD 0.178673; billed USD is unknown.

The run reached the configured 3,300-second prompt-work boundary while preserving the 3,600-second wall cap's verifier reserve. The run receipt's top-level `stop_reason` remained `end_turn` even though `terminal` was `timeout` and `continuation_stop_reason` was `turn_timeout`. S027 corrects that misleading receipt attribution. Historical safe receipts show a maximum verifier duration of 240,330 ms; the 300-second reserve remains justified.

## 4. Verification executed

### Commands and results

- Focused H007 suite: 75 tests passed.
- Focused verifier diagnostics suite: 24 tests passed.
- Focused autonomy-run suite: 61 tests passed.
- Focused autonomy cases suite: 7 tests passed.
- Assertion-quality and test-duplication guards passed.
- Python compilation and `git diff --check` passed.
- Attempt 33 preflight passed all 16 recorded checks before the provider start.
- Attempt 33 ran the unchanged independent verifier: 5 checks, 3 passed, 2 failed.

## 5. Invariant review

- Muse-only route, paired ARM64 binary/helper, task and verifier identity, request/tool/wall/disk/stall limits, cancellation rules, and 30-minute active-work criterion remained unchanged.
- Hidden verifier source, hidden test identities, assertion text, raw output, transcript, task result body, raw IDs, arguments, and credentials were not recorded in the safe evidence.
- The 300-second verifier reserve was not reduced.
- Task, verifier, and horizon outcomes remain separately recorded.

## 6. Failure and recovery review

The concrete failed sequence is bounded to `catalog_store`: its exact seed failure was eventually observed and a distinct repair test eventually completed, but the repair submission came before the failure's terminal observation and no productive edit separated those events. The current follow-up has the required per-pair projection but exposes only an aggregate cycle count and a generic requirement. S027 will surface the failed pair label and false predicates without IDs or arguments.

No provider-timeout category, event loss, job-observation loss, disk-floor event, inactivity cancellation, or daemon cleanup failure was observed. Three provider attempts failed with retry scheduled; the route reported no timeout event. All submitted jobs reached terminal observation.

## 7. Migration and compatibility review

No database, protocol, schema migration, or Rust binary change was made by S026. S027's timeout details are an additive H007-only receipt projection; other autonomy case receipt shapes remain unchanged.

## 8. Security review

The safe evidence retains no credentials, raw job IDs, arguments, transcript, task result body, hidden assertion text, or raw verifier output. Pair labels and boolean ordering predicates are public task metadata.

## 9. Documentation and operations

- Attempt 33 safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt33-safe-diagnostic-20261110.json`.
- Private receipt SHA-256: `fd9d832c583c674ac0616535e9fb4619e61eb727d1991d240939a1a585035b7d`.
- Preflight: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt33-preflight-20261110.json`.
- S027 is the next registered corrective plan.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The agent submitted the catalog-store repair before observing its seed job terminally and made no productive change in the measured interval. The generated follow-up did not identify the failing pair predicate. | One required repair pair failed; continuation invalid; verifier passed only 3/5. | S027: expose safe pair-specific ordering feedback, simplify v88's task-facing priority, and reattempt. |
| low | A caught `HarnessTimeout` left stale `stop_reason: end_turn` in the receipt. | The terminal cause is less clear than the safe `continuation_stop_reason`. | S027: set a typed `turn_timeout` stop reason and record bounded prompt timeout context. |

## 11. Roadmap disposition

Corrective implementation plan required. S026's offline prompt work is implemented; its H007 live qualification remains unachieved. S027 owns the next attempt. Do not mark S019 or the subsystem roadmap closed.

## 12. Registry updates

- Mark S026 implemented with attempt 33 unqualified and link this closure record.
- Register S027 active in `plans/registry.md` and the S019 subsystem roadmap.
- Keep S019 active with 34/120 combined starts and 11/40 supplemental starts consumed after attempt 33.
