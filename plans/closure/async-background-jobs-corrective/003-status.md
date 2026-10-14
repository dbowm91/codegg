# Async Background Jobs Corrective C003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/async-background-jobs-corrective/003-idle-lifecycle-qualification-and-closure.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md#dependency-and-execution-sequence`

Repository baseline reviewed: `f5c43f05fe127a378a4e62fe5d3c1f7eb1ee05c8`

Implementation commits or pull requests:

- `816c5054` — feat(async-jobs): C003 full idle lifecycle qualification trajectory
- `f5c43f05` — fix(async-jobs): C003 qualification repair — ci_watch on_complete copy avoids credential-surface guard substring

## 1. Executive finding

C003 is complete as a correctness/qualification slice with no product
expansion. The promised single-unit flow is proven on the implementation
branch (not merged): background work and external GitHub watch run
independent of the model; completion is persistently observable while idle;
an explicitly authorized single continuation starts without an unrelated
user turn. Default notify-only spends zero inference; cancel/expiry/
revocation suppress dispatch but retain one actionable notice; crash/race at
every persist/claim/dispatch boundary recovers without duplicate terminal
notices, remote polls, or provider dispatches; security negatives fail
closed; M001–M004 plus C001/C002 suites stay green. One bounded
corrective-only repair was required (tool-copy guard substring, below); no
second scheduler, sandbox claim, auth engine, or orchestration was added.

Implemented/qualified on branch; not merged. No merge-ready or
production-ready claim is made (exact-head hosted CI not obtained).

## 2. Requirement-to-evidence matrix

| Journey (plan §2) | Required observation | Evidence | Result |
|---|---|---|---|
| Background `test` + `bash` | Submit via real path, leave owner idle, scheduler reaches terminal, one durable attention/result, stable handle, parent works meanwhile | M002 `job_background_test_slice` 10/10 + M001 `job_notification_m001_contract` 17/17 (re-run on final head, green) | pass |
| CI watch | Real tool/daemon register, parent ends, fake GitHub → completed with no status/turn activity, daemon detects terminal within due budget, one notice, no long-lived permit | `full_idle_lifecycle_notify_only_and_granted_resume` (spawned `CiWatchIdleDriver` only; harness never calls `observe_once`/`advance_due_watches`/`status`/manual inject); `watch_counts.leased == 0` after terminal | pass |
| Default notify-only | No new provider invocation or model tokens; notice visible across two clients and after restart without manual polling | Same lifecycle test session A (NotifyOnly, 0 executor calls, frontend-B sees notice) + `restart_recovers_watches_notices_and_intents` | pass |
| Explicit consented resume | Authenticated grant + completion → one newly accepted canonical turn; fake provider enters once with bounded trusted summary + preserved permissions; nothing runs without grant | Same lifecycle test session B (`Dispatched`, 1 call, `grant_id` + session match) + C002 suite 12/12 (opt-in, 50-contender exact-once, ambiguity) | pass |
| Cancel/expiry/revocation | Cancelled owner, archived session, expired grant, budget cap, revoked principal suppress dispatch but leave actionable notice; watch cancel never cancels remote workflow | C002 `negative_lineage_cases_all_fail_closed` (8 cases) + `expired_grant_and_exhausted_budget_suppress` + C001 expiry test; M003 cancel/expiry suites green | pass |
| Crash/race | Lost notifier, shutdown/restart, insert-before-subscription, claimed-observation, terminal-before-notice, append-before-ack, claim/turn-accepted/provider-started; no duplicates; ambiguous → attention | C001 crash trio + C002 CAS/crash pair + C003 `restart_recovers_watches_notices_and_intents` (fresh handles over same pool, repair ≤1, one notice) | pass |
| Security | Forged principal/workspace, guessed ids, malicious GitHub text, disallowed redirect, invalid token, 429/5xx, sandbox Required unavailable, shell Ask/Deny preserve authority | `security_negatives_fail_closed_without_leakage` (401→attention, neutral→attention, stolen handle denied, no `Bearer`/`GITHUB_TOKEN` in notices) + C001 rate/security + C002 lineage matrix; sandbox/policy suites via quick guards | pass |
| Compatibility | M001–M004, Tool Programs M016/M017, AgentRun M009, foreground Bash/Test, session/projection/reconnect pass | 7-binary sweep green (see §4); `verify.sh quick` green; Clippy `-D warnings` green | pass |

## 3. Production implementation evidence

C003 adds no new product runtime beyond one bounded repair:

- **New test trajectory (`tests/async_background_c003_idle_lifecycle.rs`):**
  loopback GitHub fixture + SQLite `isolated_pool` + real `CiWatchService` /
  `JobNotificationService` / `ContinuationService` + spawned `CiWatchIdleDriver`
  + `IdleContinuationCoordinator` with `FakeContinuationExecutor`. Three tests:
  full two-session lifecycle (notify-only + granted resume with `Dispatched`
  asserted, two-frontend visibility, zero-lease post-terminal), restart recovery
  (drop handles, fresh pool handles, repair, one notice), security negatives
  (401/neutral/stolen-handle/no-secret-projection). The strict lifecycle path
  uses only driver wake + notice wait + coordinator dispatch — never the four
  forbidden harness drives.
- **Corrective-only repair (`f5c43f05`):** C002's `ci_watch` `on_complete`
  copy ("bounded tokens") tripped M004's `ci_watch_tool_has_no_network_or_
  credential_fields` guard (forbidden substring `token`). Reworded to
  "may start one bounded idle turn once" (no `token`/`callback`/`url`/
  `webhook`/`shell`/`gh ` substring). One-line docs change; no behavior,
  schema, or authority delta. Regression guard is the existing M004 test
  itself (now 14/14).
- **No other production delta:** C001 (`src/scheduler/ci_watch*.rs`,
  daemon singletons, migration v76) and C002 (`idle_continuation.rs`,
  migration v77, tool copy) unchanged except the one-line repair. No new
  scheduler, sandbox claim, auth engine, live runtime, or orchestration.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test --test ci_watch_m004_completion --test job_notification_m001_contract
cargo test --test job_background_test_slice --test job_continuation_m003_visibility
cargo test --test ci_watch_c001_idle_driver --test job_continuation_c002_idle_resume
cargo test --test async_background_c003_idle_lifecycle
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

Direct-binary runs (`./target/debug/deps/<binary> --test-threads=1`) used after
builds for deterministic timing where noted.

### Results

| Suite | Result |
|---|---|
| `ci_watch_m004_completion` | 14/14 pass (after `f5c43f05` repair; 13/1 fail before repair — the guard caught the regression, see §3) |
| `job_notification_m001_contract` | 17/17 pass |
| `job_background_test_slice` | 10/10 pass |
| `job_continuation_m003_visibility` | 11/11 pass |
| `ci_watch_c001_idle_driver` | 13/13 pass (5 sentinels + 8 C001) |
| `job_continuation_c002_idle_resume` | 12/12 pass (6 sentinels + 6 C002) |
| `async_background_c003_idle_lifecycle` | 8/8 pass (5 sentinels + 3 lifecycle/restart/security) |
| **Sweep total** | **85/85 pass, 0 failed** |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (`CARGO_BUILD_JOBS=2`) | pass |
| `scripts/verify.sh quick` | pass (fmt, agents check, core/client/desktop boundaries, sandbox, ADR integrity, execution-ownership, TUI authorities, HTTP disposition, audit, scheduler-bypass, config-merge, provider wire/catalog/resilience, OpenAI endpoint, Eggwork routing, skills, `cargo check --workspace --all-targets --locked`) |
| Scheduler-bypass / execution-ownership / core-boundary / auth-projection guards | pass via quick |
| Hosted `CI / verify` + Desktop E2E on exact head `f5c43f05` | **not run** — no workflow ID/SHA/conclusion to record; merge-ready explicitly not claimed (see §10) |
| Native macOS/Windows, live GitHub API, paid live provider | **not run** — unavailable in this environment; labelled, not fabricated |

## 5. Invariant review

- One scheduler admission authority; watch reads hold no permits; single idle
  driver per daemon: pass — `leased == 0` post-terminal; driver is one loop.
- Singleton daemon services; closing a turn leaves watchers live: pass —
  C001 singleton test green on final head.
- Notify-only default spends zero inference; no silent tool execution: pass —
  session A 0 calls; C002 default test green.
- Autonomous continuation only via explicit grant with bounded budgets: pass —
  session B 1 call with grant match; grant-less stays notify-only.
- Auth bound to immutable lineage; guessed handles denied without oracle:
  pass — stolen-handle + lineage-matrix tests green.
- Credential refs only; allowlist/redirect/response/rate-limit preserved:
  pass — no `Bearer`/`GITHUB_TOKEN` in notices; 401/neutral → attention.
- No unbounded poller, no duplicate observation/dispatch, no blind replay:
  pass — 50-contender exact-once on both legs; ambiguous → attention.

## 6. Failure and recovery review

- Duplicate delivery: M001 idempotent notices + C001 single-admission + C002
  first-write-wins; full sweep shows one notice and one turn per watch/grant.
- Cancellation races: watch cancel stops watching only; grant revoke/expiry/
  budget and owner-idle/provider gates suppress before invocation.
- Restart: C003 restart test (drop → fresh handles → repair → driver → one
  notice); C001/C002 restart trios green on final head.
- Partial persistence: insert-before-subscription repaired; conflicting owner
  fails to attention; `TurnAccepted` persists before dispatch.
- Stale generation/lease: C001 CAS + DB-authoritative probe; C002 intent CAS;
  late duplicates `AlreadyTerminal`/`AlreadyDispatched` with no re-poll.
- Contention: bounded batches (driver 8/tick, turn boundary 2), fair
  due-order, 50-contender proofs on both legs.
- Malformed/unauthorized: 401/neutral/redirect/guessed-id/malicious-text all
  attention-or-denied; shell Ask/Deny and sandbox Required paths unchanged
  (policy suites via guards).
- Bounded events/artifacts: notice summaries bounded/redacted; no secret
  projection.

## 7. Migration and compatibility review

- Schema: v76 (C001 lease) + v77 (C002 grants/intents) additive only;
  `STORAGE_LAYOUT_VERSION` 77 tracks highest migration (catalog-invariants
  guard green). No C003 migration. Rollback to v75/v76 leaves newer columns/
  tables unread with idle safely degraded to notify-only; terminal notices
  idempotent.
- Protocol/config: `ci_watch` schema unchanged by C003 except the one-line
  `on_complete` copy fix (no model-shape change); no config keys added.
- Compatibility: M001–M004, C001/C002, Tool Programs, AgentRun, foreground
  Bash/Test, session/projection/reconnect all green per §4. Original M001–
  M004 closures untouched. Windows native sandbox line untouched (still
  independently blocked; no assurance expanded here).

## 8. Security review

- Exact-owner enforced on watches/notices/grants; stolen/guessed ids denied
  as not-found; observer authorizes nothing.
- Grant is the sole idle-autonomy root (host-resolved ids, lineage re-bound
  at dispatch, expiry/budget/revocation enforced); model flag alone dispatches
  nothing.
- Remote CI output stays inert data; only compact host refs enter authorized
  context; run URLs validated same-origin before embedding.
- Sandbox Required unavailable stays fail-closed; shell Ask/Deny ceilings
  preserved; no privilege expansion; secrets never persisted/logged
  (projection scan: no `Bearer`, no `GITHUB_TOKEN`).
- Two SQLite handles/claimants proven single-winner on both legs; budgets
  prevent starvation; per-session caps preserved.

## 9. Documentation and operations

- This record is the only C003 artifact plus the `f5c43f05` one-line repair;
  no architecture doc needed changes beyond C001/C002 (scheduler/jobs/agent/
  authorization already freeze the driver + grant contracts).
- Operator: standard `watch_counts`, driver `wake`/`request_shutdown`,
  `repair_registrations` startup log, grant `authorize`/`revoke`, intent
  `intents_for_notice`/`get_intent`; user cancel = revoke (wins pre-invoke).
- Static guards all green via quick; no new guard required (M004 guard caught
  the one regression).
- Branch status: implemented/qualified on branch `work/async-background-jobs-
  idle-completion-corrective-plans` at `f5c43f05`; not merged; no PR opened
  by this closure.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Exact-head hosted `CI / verify` + Desktop E2E not run (`f5c43f05` local-only) | No hosted qualification; merge-ready not claimed | Separate integration decision must request/run hosted workflows on the merge SHA and record IDs before any merge |
| low | Native macOS/Windows, live GitHub API, paid live provider not exercised | Platform/live-network qualification absent | Labelled unavailable; do not fabricate; future platform/lane work owns it |
| low | C003 sweep re-captures M004/background exact counts (previously tail-truncated in C001 closure) | Closes the C001 low precision gap | Done in §4 (14 + 10 recorded) |

No critical/high/medium findings. All strict gates pass locally:

- Idle CI-watch terminal from a real daemon coordinator: pass.
- Notify-only zero-inference: pass.
- Explicit-grant single normal turn: pass.
- No manual `status`/new-turn drive on the positive path: pass.
- No unbounded poller, double observation, orphan watch, cross-session
  notice, unauthorized turn, duplicate dispatch, or blind replay: pass.
- No merge-ready claim without hosted CI: honored (none made).

## 11. Roadmap disposition

Workstream **complete**: C001 closed, C002 closed, C003 closed. The broader
"mcode-style idle completion → notify/authorized resume" use case is now
qualified on the branch to the deterministic evidence above. No further
milestone is planned in this corrective line; its status text is reconciled
to terminal truth. Original M001–M004 history remains unchanged.

## 12. Registry updates

- `plans/registry.md`: move C003 `ready` → `closed` (closure
  `plans/closure/async-background-jobs-corrective/003-status.md`,
  implementations `816c5054` + repair `f5c43f05`); roadmap row `active (C001
  closed; C002 closed; C003 ready)` → `closed (C001–C003 closed)`; unblock
  audit: nothing newly unblocked — no registered plan lists C003 as a hard
  or interface dependency (verified against Blocked-work table and subsystem
  dependency graphs).
- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md`:
  status → `closed — C001/C002/C003 closed`; milestone table C003 `ready` →
  `closed` with closure link.
- `plans/implementation/async-background-jobs-corrective/003-*.md`: `ready for handoff` → `implemented` (done in implementation commit; repair in `f5c43f05`).
