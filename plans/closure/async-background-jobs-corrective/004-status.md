# Async Background Jobs Corrective C004 — Closure Status

Status: corrective pass required

Source implementation plan:

- `plans/implementation/async-background-jobs-corrective/004-production-idle-dispatch-and-attention-delivery.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md#post-c003-production-wiring-gap-and-successor-c004`

Repository baseline reviewed: `4155a0f9798b79947f4969af230cd183e2381616`

Implementation commits or pull requests:

- `fcbb915a` — feat(async-jobs): C004 daemon-owned idle completion delivery with live owner event

## 1. Executive finding

C004 lands the delivery half of its objective as a bounded production
corrective with no product expansion, and explicitly leaves the autonomous
resume half blocked per the plan's own stop rule (§1) and failure
disposition (§6). A submitted background job or CI-watch-shaped wait now
ends after its originating turn; the running daemon durably records exactly
one terminal notice and publishes one persisted owner-scoped live attention
update with no further user/model/tool requests and zero model tokens.
Default `notify_only` never calls a provider; grants alone dispatch
nothing. What is NOT delivered: no authenticated host-grant workflow
exists (`ContinuationService::authorize` still takes caller-supplied
strings), no production coordinator/executor is wired, and no delegated
turn-admission entry exists — so the daemon never admits an idle provider
turn. That resume half is registered as blocked successor C005
(`plans/implementation/async-background-jobs-corrective/005-authenticated-idle-resume-and-canonical-executor.md`)
rather than improvised with a model flag or fake client identity.

Implemented on the plan branch; not merged. No merge-ready or
production-resume claim is made (exact-head hosted CI/E2E not obtained;
`main` divergence not reconciled).

## 2. Requirement-to-evidence matrix

| Requirement (plan §5) | Evidence | Result | Notes |
|---|---|---|---|
| 1. Prod idle resume via real daemon + host grant + real `TurnRuntime`, no harness dispatch | `continuation_store_owned_but_idle_resume_stays_blocked`: grant + completion through the real daemon yields a live event and zero `TurnStarted`/`TurnCompleted`/`TurnFailed`; no `maybe_dispatch`/`TurnSubmit` in the file | blocked | No host grant path, executor, or delegated admission exists; fail-closed proven instead of faked |
| 2. True notify-only (default/absent/invalid/model-only grant): one notice + one live event, zero turns | C004 live-attention tests (owner delivery, watch fanout, blocked-resume) + C002 `negative_lineage_cases_all_fail_closed` + M001/M003 suites | pass | Permission denial before work covered by existing suites; delivery spends zero inference |
| 3. Live+replay: two-client scope, disconnect/reconnect, restart, typed projection consumption | `daemon_owns_singletons_and_publishes_live_completion_to_owner` (live envelope + owner/foreign replay filters + zero-turn proof); `watch_shaped_terminal_shares_single_fanout_and_router_hint`; `replay_survives_daemon_restart_without_second_turn` (fresh daemon, same pool, identical notice id/status) | pass | Owner scoping proven at `EventLog` filter/replay layer shared with live transport; transport-level two-socket isolation relies on existing authorization/projection tests (not re-proven here) |
| 4. Consent/revocation matrix prevents idle provider start with actionable attention | Grant `authorize`/`revoke` store paths exercised; C002 8-case lineage matrix + expiry/budget/suppression suites green; production prevention is vacuous-fail-closed (no dispatch path) | partial | Store-level matrix passes; per-case daemon provider-start prevention unproven because no provider-start path exists (blocked, see §10) |
| 5. CAS/crash (50 races, restart before/after every boundary, exactly-once, ambiguity → attention) | `service_fanout_fires_once_for_first_write_only` + duplicate-terminal suppression in both live tests + restart replay + prior C001/C002 50-contender proofs for notice/intent legs | partial | Live-fanout 50-race and crash-before/after projection/intent/turn/provider matrix not run at C004 scale; duplicates + restart covered (see §10 medium) |
| 6. Interoperability (C003 §5 item 6 set) | 90-case sweep green (see §4) + `verify.sh quick` | pass | No foreground semantic change |
| 7. Host evidence (fmt/clippy/quick/guards + hosted CI/E2E) | `cargo fmt --check`, Clippy `-D warnings`, `verify.sh quick`, scheduler/execution-ownership/core-boundary/auth-projection guards pass locally; hosted `CI / verify` + Desktop E2E not run; Windows/macOS/live provider/real GitHub not run | partial | Local gates pass; hosted/integration explicitly not claimed (see §10 low) |
| 8. Mainline integration (divergence reconciled, gates rerun) | Not run | blocked | Branch descends from `716cfece`; `main` diverged independently; no integration PR/head (operational, see §10) |

## 3. Production implementation evidence

Delivered (fail-closed, no new authority):

- **Additive live event:** `CoreEvent::BackgroundJobCompleted { session_id, notice_id, job_id, kind, status, summary }` (`crates/codegg-protocol/src/core.rs`); `core_event_type = background_job_completed` (`src/core/mod.rs`); persisted via `should_persist` (`src/core/event_log.rs`); `Safe` publication + safe origin (`crates/codegg-core/src/projection_replay/safe_publication.rs`); ephemeral `NotificationKind::BackgroundJobCompleted` hint (`src/core/notification.rs`, parse + plural + verb).
- **Single post-commit fanout:** `JobNotificationService::record_notice` sends only first-write terminals over a daemon-installed unbounded channel after the durable commit (`src/scheduler/job_notifications.rs`: `completion_tx`, `set_completion_hook_sync`, `emit_completion`); idempotent duplicates return early without sending; `reconcile_terminals` backfills through the same path. Machine jobs (`record_terminal` via scheduler terminal path) and CI watches (`record_terminal_for_watch` via `CiWatchIdleDriver` terminal path) share this one fanout; no per-job task.
- **Daemon ownership:** `CoreRuntimeDeps::shared_continuation_service` + `continuation_service` field, `CoreDaemon::continuation_service` + `_idle_completion_handle` (`src/core/{runtime_deps,daemon}.rs`); `with_deps_and_identity` creates the continuation singleton, installs the channel, and spawns `CoreDaemon::forward_idle_completions` (`src/core/daemon_construct.rs`); the forwarder publishes one persisted session-scoped live event plus one dedupe-keyed router toast per notice and never starts a turn (`src/core/daemon.rs`); shutdown aborts the forwarder while committed notices stay durable (`src/core/daemon_shutdown.rs`).
- **Fail-closed resume:** the daemon wires no coordinator/executor and never calls `maybe_dispatch` in production; `ContinuationService` rows stay non-autonomous; every completion is notify-only attention.
- **Docs:** `architecture/{agent,jobs,scheduler,authorization,projection}.md` record the delivery contract and the explicitly blocked resume.

Not delivered (blocked, see §10): authenticated grant `CoreRequest` family + `operation_descriptor`/audit wiring; production `ContinuationExecutor` + delegated turn admission; automatic terminal-to-dispatch wake.

No new scheduler, sandbox claim, auth engine, messaging platform, or orchestration. Historical M001–M004/C001–C003 closures untouched.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test --test async_background_c004_live_attention --test ci_watch_m004_completion --test job_notification_m001_contract --test job_background_test_slice --test job_continuation_m003_visibility --test ci_watch_c001_idle_driver --test job_continuation_c002_idle_resume --test async_background_c003_idle_lifecycle
cargo test -p codegg-core --lib projection_replay::safe_publication
python3 scripts/check_authorization_matrix.py
python3 scripts/check_audit_coverage.py
bash scripts/check-core-boundary.sh
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

Direct-binary runs were not needed; `cargo test` ordering was sufficient (no timing-sensitive flake observed).

### Results

| Suite | Result |
|---|---|
| `async_background_c004_live_attention` | 5/5 pass (10/10 with `common::secret_scan`; ~1.8s) |
| `ci_watch_m004_completion` | 14/14 pass |
| `job_notification_m001_contract` | 17/17 pass |
| `job_background_test_slice` | 10/10 pass |
| `job_continuation_m003_visibility` | 11/11 pass |
| `ci_watch_c001_idle_driver` | 13/13 pass |
| `job_continuation_c002_idle_resume` | 12/12 pass |
| `async_background_c003_idle_lifecycle` | 8/8 pass |
| **Focused sweep total** | **90/90 pass (14 + 17 + 10 + 11 + 13 + 12 + 8 + 5)** |
| `codegg-core` `safe_publication` lib | 3/3 pass |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (`CARGO_BUILD_JOBS=2`) | pass |
| `scripts/verify.sh quick` | pass (fmt, agents, core/client/desktop boundaries, sandbox, execution-ownership, TUI authorities, HTTP disposition, audit, scheduler-bypass, config-merge, provider wire/catalog/resilience, OpenAI endpoint, Eggwork routing, skills, workspace check) |
| Scheduler-bypass / execution-ownership / core-boundary / authorization-matrix / audit-coverage guards | pass (also via quick) |
| Hosted `CI / verify` + Desktop E2E on `fcbb915a` | **not run** — no workflow ID/SHA/conclusion; merge-ready explicitly not claimed |
| Native macOS/Windows, live GitHub API, paid live provider | **not run** — unavailable; labelled, not fabricated |
| Mainline integration head | **not run** — no integration PR/merge SHA |

## 5. Invariant review

- One scheduler admission authority; watch/job terminals hold no permits; single idle driver + single fanout per daemon: pass — driver batch/sleep bounds unchanged; fanout is channel + one forwarder, no per-job task.
- Singleton daemon services; closing a turn leaves watchers/delivery live: pass — pointer-identity test for notifier + continuation store; `TurnRunInput` shared handles unchanged.
- Notify-only default spends zero inference; no silent tool execution: pass — delivery tests assert zero `TurnStarted`/`TurnCompleted`/`TurnFailed` for the session.
- Autonomous continuation only via explicit host grant: pass vacuously — no production dispatch path exists, so nothing dispatches without (or with) a grant; model `resume_owner` alone stays notify-only.
- Exact-owner delivery; guessed handles denied without oracle: pass — owner/foreign replay filters; cross-session forgery still denied by M001/C002 suites.
- Credential refs only; bounded redacted summaries; no secret projection: pass — live payload carries identity/kind/status/summary only; tests assert no `Bearer`/`GITHUB_TOKEN`.
- No unbounded poller, no duplicate observation/dispatch, no blind replay: pass for delivery — first-write-wins fanout + idempotent `record_notice` + persisted replay; provider-replay N/A (blocked).

## 6. Failure and recovery review

- Duplicate delivery: first-write-wins notice + fanout-on-first-write-only + live duplicate-suppression tests + persisted replay deduped by notice id.
- Cancellation races: terminal `cancelled`/`expired`/`needs_attention` statuses still produce attention-only notices; grant revoke/expiry/budget paths covered by C002 suites; production provider-start races N/A (no dispatch).
- Restart: notice row durable; live event persisted in `core_event_log`; fresh daemon over the same pool replays the identical notice id/status with no new turn (proven). Crash between notice commit and channel send is narrowed to two synchronous ops; a lost hint falls back to the notice row + next-turn session event (documented low gap, no blind retry).
- Partial persistence: conflicting terminal digest stays `Conflict` with first-winner retained, no fanout on conflict.
- Stale generation/lease: C001 CAS + DB probe unchanged; continuation intents untouched by C004.
- Contention: fanout channel unbounded but each terminal sends at most one small clone; forwarder is one task; no long-held locks across send (sync `try`-free `std::RwLock` read).
- Malformed/unauthorized: empty session fails closed (`Storage`); missing subscription fails closed (`Unauthorized`); forged session denied by existing suites.
- Bounded events/artifacts: summary already bounded at record (`max_summary_chars` 2048); live payload reuses it without expansion; router toast dedupe-keyed by notice id.

## 7. Migration and compatibility review

- Schema: no migration. `STORAGE_LAYOUT_VERSION` unchanged (catalog-invariants guard green via quick). `core_event_log` reuses the existing table with a new `event_type` string; old rows replay unchanged.
- Protocol/config: one additive `CoreEvent` variant with plain `String` fields (version-compatible; old daemons report it as `other` via the `core_event_type` wildcard and `Safe` fallback does not apply — new variant is explicitly classified). No `CoreRequest`/`CoreResponse`/config change.
- Compatibility: M001–M004, C001–C003, Tool Programs, AgentRun, foreground Bash/Test, session/projection/reconnect all green per §4. No second completion database (router toast ephemeral, dedupe-keyed). `SessionEvent::JobTerminalNotification` path untouched, so no double-append (idle path never appends session events).

## 8. Security review

- Exact-owner enforced on notices; live envelope carries the same owning `session_id`; foreign filters observe nothing.
- Grant is NOT a production autonomy root in this slice: `authorize` remains a caller-string boundary, documented as unauthenticated; the daemon never dispatches from it. No model flag, raw frontend metadata, unauthenticated socket, or fake `client_id` was substituted for verification (no dispatch at all).
- Remote CI/job output stays inert data; only the bounded host-stored summary travels in the live hint; run URLs are not embedded in the new event.
- Sandbox Required unavailable stays fail-closed; shell Ask/Deny ceilings preserved; no privilege expansion; secrets never persisted/logged (projection scan in tests).
- One daemon fanout per process; duplicate/restart proofs show single-winner live delivery; budgets prevent starvation; per-session caps preserved.

## 9. Documentation and operations

- `architecture/agent.md`: C004 delivery slice + blocked resume.
- `architecture/jobs.md`: production idle delivery contract, fanout, event, blocked resume.
- `architecture/scheduler.md`: continuation store + fanout ownership, no per-job task.
- `architecture/authorization.md`: grant boundary still unauthenticated; production dispatch blocked; no flag/client forgery.
- `architecture/projection.md`: additive live hint, replay/restart semantics, notice row stays authoritative.
- Operator: standard `watch_counts`, driver `wake`/`request_shutdown`, `repair_registrations`, grant `authorize`/`revoke`, notice `pending_for_session`/`get`; live health is the `BackgroundJobCompleted` envelope plus the `bg-<notice_id>` router hint; user cancel = revoke (wins pre-invoke when dispatch exists; currently all completions are attention-only).
- Static guards all green via quick; no new guard required (existing M004 guard surface untouched).
- Branch status: delivery implemented at `fcbb915a` on `work/async-background-jobs-production-wiring-c004-plans`; not merged; no PR opened by this closure.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No authenticated host-grant workflow; `NewGrant` lineage still caller-supplied; no production coordinator/executor or delegated turn admission | Autonomous idle provider resume unavailable; every completion is attention-only by design | Successor C005 (`plans/implementation/async-background-jobs-corrective/005-authenticated-idle-resume-and-canonical-executor.md`, blocked): server-resolved grant family + canonical executor + automatic wake; never substitute model flag/fake client |
| medium | Full 50-race + crash-before/after matrix not run at live-fanout scale (only 2x duplicate + restart replay + prior 50-contender notice/intent legs) | Bounded residual risk of duplicate hint under extreme contention | C005 (or future hardening) runs the racing/restart sweep at fanout scale if resume lands; current first-write-wins + idempotent replay bounds the delivery half |
| low | Exact-head hosted `CI / verify` + Desktop E2E not run (`fcbb915a` local-only) | No hosted qualification; merge-ready not claimed | Separate integration decision must run/record hosted workflows on the merge SHA before any merge |
| low | Native macOS/Windows, live GitHub API, paid live provider not exercised | Platform/live-network qualification absent | Labelled unavailable; future platform/lane work owns it |
| low | TUI/desktop rendering of `BackgroundJobCompleted` not asserted beyond `EventLog` + seam publication | Presentation gap only; durability/delivery proven at the daemon/event layer | Frontend slice (if desired) asserts the typed event renders an actionable handle via existing authorized controls |
| low | Crash between notice commit and channel send has no deduped startup republish (falls back to notice row + next-turn session event) | Narrow synchronous window; live hint could miss while durability holds | Future hardening adds startup reconciliation with `core_event_log` dedupe by notice id if the window matters operationally |

No critical findings. The high finding is the plan-anticipated blocked resume, not a delivery defect.

## 11. Roadmap disposition

Workstream stays **active**: C004 delivery is landed and qualified to the evidence above, but the milestone as written (delivery + autonomous resume) is NOT closed. Disposition: **corrective pass required** — successor C005 (authenticated idle resume + canonical executor) is registered as blocked and owns the remaining resume half. C001–C003 closures remain historical; the end-to-end production-resume claim stays suspended until C005 closes. Original M001–M004 history unchanged.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps: async corrective row `active (C004 ready; C001–C003 historical closures)` → `active (C004 delivery landed, resume blocked with C005; C001–C003 historical closures)`.
- `plans/registry.md` Dependency-ready implementation plans: C004 `ready` → `corrective pass required (delivery landed at fcbb915a; resume blocked; closure 004-status.md)`; add C005 `blocked` row pointing at `005-authenticated-idle-resume-and-canonical-executor.md`.
- `plans/registry.md` Blocked work: add C004-resume/C005 entry (blocked on authenticated host-grant workflow + delegated turn admission, possible ADR).
- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md`: status stays `active`; milestone table C004 `ready` → `corrective pass required (delivery landed, resume blocked)` with closure link; add C005 `blocked` row; dependency graph extended C004 → C005.
- `plans/implementation/async-background-jobs-corrective/004-*.md`: `ready for handoff` → `partially implemented (delivery landed, resume blocked)` (status line edit in place; requirements unchanged).
- Unblock audit: nothing newly unblocked — no registered plan lists C004-delivery or C005 as a hard/interface dependency (verified against Blocked-work table and subsystem dependency graphs). C005 is newly registered as blocked, not ready.
