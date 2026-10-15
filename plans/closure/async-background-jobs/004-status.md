# Async Background Jobs M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/async-background-jobs/004-github-actions-completion-watch.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md#m004--github-actions-remote-ci-terminal-watcher`

Repository baseline reviewed: `6860aec997e16dc685e6a59c1cdfb55a8a117b4e`

Implementation commits or pull requests:

- `6860aec9` — feat(async-jobs): M004 GitHub Actions completion watch (18 files, code + tests + docs)

## 1. Executive finding

M004's capability is complete: a canonical daemon-owned watch operation registers one authorized immutable GitHub run/commit/check identity, returns a stable watch handle, observes completion without repeated model `gh run view`/sleep/loop calls, and reports a single actionable terminal result through M001/M003. The model may keep working or end the turn under the M003 continuation policy. This is not a remote process execution job and holds no CPU/process permit across polls; remote observation never occupies scheduler capacity. With M001–M003 closed, the full async-background-jobs line is now closed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A. Identify existing GitHub API/token transport; freeze `CiWatchTarget`, creation/subscription idempotency, status/projection/control response, resume policy; run id + optional SHA/check; decline ambiguous/untrusted/unsupported; loopback mocks only | `src/scheduler/ci_watch.rs::{CiWatchTarget::validate, create_watch}` (coalescing `ciw-<digest>` ids); existing `eggfetch_core::Client` via trusted builder; `tests/ci_watch_m004_completion.rs::{target_validation_rejects_garbage_before_any_io, same_invocation_coalesces_independent_watches_distinct, negative_matrix_never_leaks_nor_reports_false_success}` | pass | Unsupported providers never probed (request log unchanged); ambiguous zero/multi matches report counts, never success |
| B. Durable watch record (identity, state, next_check_at, etag, budget, expiry, owner, conclusion, SHA/run); ScheduleStore-or-narrow-adapter decision documented; migration + restart/reconciliation; no second timer/queue | `ci_watch` table (migration v75, layout 75); `src/scheduler/ci_watch.rs` header + `architecture/scheduler.md` authority section (ScheduleStore recurring semantics rejected with reasons); `restart_overlap_duplicate_cancel_terminal_before_create_backoff` (fresh service sees watch + notice) | pass | Narrow adapter holds all timing/budget state; observations only at daemon-owned points |
| C. Canonical URLs + project remote association; trusted HTTP/TLS/credential path; least-privilege read token; error codes, pagination, size/rate limits; fail closed on no credential/scope-mismatch/404-ambiguity/malformed/unsupported GHE | `check_url_against_allowlist` (scheme/host/port, userinfo reject, loopback rule), `follow_redirects(false)` + `check_same_origin_redirect` (same-origin only, 3 hops), 256KiB cap, 15s timeout, `credential_ref_for_request`/`resolve_github_token` (env only, never stored/logged), `per_page` bounds, jobs page fetched once at terminal failure | pass | GHE hosts require explicit allowlist; mixed-host fleets out of scope (documented) |
| D. Authoritative `completed` → persist final → one M001 notice; M003 idle/authorized delivery; duplicates/restarts/cancel-races → single notice, no terminal re-poll; nonterminal → projections only; validated run URL, bounded failing-step summaries, SHA/status refs; API failure ≠ CI failure | `apply_run_observation` (persist-first ordering) + `emit_terminal_notice` (idempotent `ciwatch:<id>`) + `AlreadyTerminal` guard; full conclusion map incl. neutral/skipped/stale/unknown → attention; `failure_conclusion_maps_with_bounded_step_summary` (500-char cap, inert text) | pass | Cancelled watches still yield one attention notice (consistent with jobs); user cancel wins over resume via M003 suppress |
| E. One small tool (`ci_watch {repo, run_id, on_complete}`, `status\|cancel`, plus bounded `list`); explicit run ids, uniqueness-verified commit resolution; poll-free guidance; TUI bounded status + cancel | `src/tool/ci_watch.rs` (no URL/token/callback/shell fields — asserted by `ci_watch_tool_has_no_network_or_credential_fields`); commit resolution requires exactly-one match; `architecture/projection.md` precedent (existing surfaces, no polling task) | pass | TUI renders session events (notices) as today; watch status/cancel/list via tool — same disposition as M003, no new TUI polling |
| F. Docs (jobs/scheduling/auth/projection, credential config, limits, uncertain states) + roadmap/registry | `architecture/{jobs,scheduler,authorization}.md` sections; this closure; registry updates below | pass | Env config documented (`CODEGG_GITHUB_API_ALLOWLIST`, `GITHUB_TOKEN`/`GH_TOKEN`); schema keys deferred (low finding) |
| Acceptance: fake run queued→in_progress→completed (+failure); one terminal event + optional authorized continuation; no model CI calls | `fake_run_progresses_to_terminal_success_with_one_notice`, `failure_conclusion_maps_with_bounded_step_summary`; harness drives `observe_once` directly | pass | Resume path covered by shared M003 lease (watch `on_complete` registers `ciwatch:<id>` subscription + lease; M003 single-consume tests green) |
| Acceptance: coalescing, distinct ownership, restart, overlap, duplicates, cancel-before-terminal, terminal-before-creation, 429/503 | `same_invocation_coalesces…`, `restart_overlap_duplicate_cancel_terminal_before_create_backoff` (Busy on overlap, AlreadyTerminal + zero new requests after terminal, cancel → one notice, pre-completed fixture → immediate terminal, Retry-After honored, 503 transient) | pass | — |
| Acceptance: dormant watcher holds no process/permit; observations separately admitted/bounded; quota/rate/TTL guard SBC hosts | Creation performs zero network I/O (asserted); per-watch mutex (one occurrence); session cap 8; host budget via attempt budget + backoff; TTL default 2h/max 12h; `dormant` proven by empty request log + no spawned tasks (no `tokio::spawn` in watch paths) | pass | Turn-boundary driver capped at 2 observations |
| Acceptance: 401/403/404, repo mismatch, SHA drift, invalid id, ambiguous check, GHE redirect, SSRF, stolen handle → no leak, no false success; neutral/skipped ≠ passed | `negative_matrix_never_leaks_nor_reports_false_success`, `sha_drift_and_repo_mismatch_fail_closed`; stolen handle → `NotFound` (same as missing); unsupported → zero new requests | pass | 404 ambiguity never distinguished publicly; userinfo URLs rejected |
| Acceptance: cancel ≠ remote cancel; URLs verified inert links | `cancel` test (state cancelled, remote untouched by construction — no remote call exists); `validated_run_url` same-origin gate, `None` on mismatch | pass | — |
| Acceptance: prior Tool Program/AgentRun/scheduler suites green; local fixtures only | M016 (2), M017 (2), `scheduler_restart_recovery` (15), M001 (17+10), M002 (12), M003 (11) all green; no new workflow/credential service/webhook/listener | pass | — |

## 3. Production implementation evidence

- `src/scheduler/ci_watch.rs` (~1,300 lines): target validation, `CiWatchRecord`/`CiWatchView`, `CiWatchConfig` (`from_env` allowlist, `test_with_base_url`), `CiWatchService` (memory + SQLite CAS, per-watch in-flight mutex, session quota, deterministic digest ids, deterministic jitter), full observation pipeline (resolve → fetch → apply; ETag/304, 429/Retry-After, 3-hop same-origin redirects, 401/403/404/sha-drift/ambiguous → attention, transient → budgeted backoff, expiry/budget → expired), terminal-first persistence + idempotent M001 notice, cancel/list/status (status advances when due), turn-boundary `advance_due_watches` (cap 2).
- `src/scheduler/job_notifications.rs`: `record_terminal_for_watch` (subscription-proof check, idempotent `ciwatch:<id>`, non-final rejection) + `needs_attention` message arm (attention, never "CI failed").
- `crates/codegg-core/src/session/schema.rs` + `storage/mod.rs`: migration v75 (`ci_watch` + indexes, `ON CONFLICT(watch_id) DO NOTHING` semantics preserved), layout 74→75.
- `src/tool/ci_watch.rs` + registry/factory/permission/loop wiring: `create|status|cancel|list`, exact-owner scope, `ShellExec` agreement, turn-runtime construction (pool + M001 notifier + env config), `follow_up` boundary advancement before notice injection (terminal observations on this boundary flow into the same turn's injection).
- Credential/URL authority: base URL from daemon env config only (never model input); tokens from process env per poll (never rows/logs); GitHub output treated as data; step names/summaries inert bounded text.
- Absent by design: webhook listener, cron/DAG, multi-provider CI, arbitrary URL watches, perpetual recurrence as CPU job, plaintext credential storage, long-lived pollers, fabricated identities, transient-error-as-CI-failure, `neutral/skipped`-as-passed.

## 4. Verification executed

### Commands run

```bash
cargo test --test ci_watch_m004_completion
cargo test --test scheduler_restart_recovery --test tool_program_m016_notification_replay --test tool_program_m017_notification_confirmation
cargo test --test job_notification_m001_contract --test job_background_test_slice --test bash_background_m002_routes --test job_continuation_m003_visibility
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
bash scripts/check-core-boundary.sh
```

### Results

- `ci_watch_m004_completion`: 14 passed (9 new + 5 shared `common::secret_scan`), 0 failed. Entirely loopback fixtures; no hosted API qualification asserted (see §10).
- `scheduler_restart_recovery`: 15 passed. M016: 2 passed. M017: 2 passed.
- M001: 17 + 10 passed. M002: 12 passed. M003: 11 passed.
- `cargo fmt --check`: pass. `clippy --workspace --all-targets -- -D warnings`: pass (same `--all-features` deviation as M001–M003 per repo policy; test-only `dead_code`/`manual_split_once` cleanups in-tree).
- `scripts/verify.sh quick`: **pass** (incl. agent schema, boundaries, sandbox, execution-ownership, TUI authority, http-route-disposition, audit-coverage, scheduler-bypass, config-merge-coverage, no-hardcoded-models, provider wire/catalog/resilience, OpenAI endpoint, Eggwork routing, workspace check).
- Guards: scheduler-bypass ok, execution-ownership ok (no manifest change — no new execution owner), core-boundary pass.
- Local-only platforms: Windows/macOS not exercised; hosted CI/Desktop E2E not run (low findings, recorded not waived).
- Stop conditions not triggered: no plaintext credentials stored, no arbitrary network targets (allowlist gate), no permanent pollers (activity-gated observations), no transient-as-failure, no fabricated identities.

## 5. Invariant review

- One canonical daemon `watch_ci` per explicit immutable identity; stable handle; model works or ends turn under M003 policy: `create_watch` digest ids + `ci_watch` tool + M003 `on_complete` passthrough.
- Not a remote execution job; no CPU/process permit across polls: observations are inline bounded GETs under a mutex; scheduler capacity untouched; dormant watches are rows only (zero-I/O creation asserted).
- Immutable target binding (owner/repo/run or SHA+selector), generation/time window (TTL), endpoint host + credential ref: frozen row fields; SHA re-verified at resolve and fetch (drift → attention); credential is a ref, resolved per poll.
- Ambiguous multiples never unique success: zero/multi → `Ambiguous` → attention with counts (tests).
- Explicit states incl. `needs_attention`; queued/in_progress nonterminal; all GitHub conclusions mapped; watch timeout = expiry/attention, never remote cancel: state machine + mapping tests.
- Allowlisted base URLs, TLS, credential refs at dispatch, redirect/SSRF rejection, bounded sizes: `check_url_against_allowlist`, `follow_redirects(false)`, same-origin hop validation, 256KiB cap; no model callback/shell/token surface (schema assertion test).
- Finite scheduled observations via ScheduleStore-or-adapter decision (narrow adapter chosen with documented rationale), one active occurrence per watch, per-principal/project caps (session cap 8), host budgets (attempt budget + backoff), 403/429 Retry-After + ETag: implementation + tests.
- Zero idle pollers; no per-watch interval/`gh run watch`/agent loops; no perpetual recurrence as CPU job: grep-verifiable (no `spawn`/`interval` in watch paths); creation performs zero I/O (asserted).
- Persist-before-notify + claim/ack/replay (M001), stale claims, bounded backlog: reused verbatim (M001–M003 suites green); duplicate/restart/cancel races single-notice (tests).
- `Interrupted`/retry vs final: N/A to watches (no attempts); watch-level equivalents — transient vs terminal-attention vs expired — classified per table above.
- No default idle auto-resume: `on_complete` default `notify_only`; resume via M003 single-use lease only.
- Per-session handles/notices/watchers/retry-poll budgets, wake concurrency (boundary cap 2), continuation count (lease 1) bounded; no busy-poll loop, duplicate scheduler, permanent CI lane, unbounded provider requests: config + tests.

## 6. Failure and recovery review

- Duplicate delivery/idempotency: `ciw-<digest>` creation coalescing + `ciwatch:<id>` notice idempotency + lease CAS; duplicate terminal responses → `AlreadyTerminal`, zero new requests, one notice (tests).
- Cancellation races: user cancel → `Cancelled` + one notice + never re-polled; remote completion racing cancel resolves by durable state precedence (first terminal wins; loser sees `AlreadyTerminal`); no remote workflow call exists to misfire.
- Daemon/node restart: SQLite watch/notice/subscription/lease rows; fresh service resumes (restart agreement test); `next_check_at`/backoff/ETag durable across restart.
- Partial persistence failure: terminal persisted before notice; notice failure leaves terminal row for next-boundary retry (`emit_terminal_notice` re-reads authoritative row); never re-executes remote completion.
- Stale generation/lease: TTL expiry → `Expired` + notice; M003 lease expiry suppresses resume; re-observe of terminal → `AlreadyTerminal`.
- Contention/resource release: per-watch mutex (Busy skips); boundary cap 2; session quota; no permits held; scheduler admission untouched.
- Malformed/unauthorized input: validation rejects pre-I/O (unit tests); forged sessions/stolen handles → `NotFound`; userinfo/cross-origin/mixed-content URLs rejected; principal forgery impossible (daemon-resolved).
- Bounded event/artifact behavior: 500-char failure summaries, validated URLs only, 32-list cap, 256KiB bodies, 50-200 attempt budgets, 2h/12h TTLs.

## 7. Migration and compatibility review

- Migration v75 additive (`IF NOT EXISTS` + indexes, `UNIQUE` target identity); no historical row modified or backfilled. Layout 74→75 tracks highest migration; `verify.sh quick` green.
- Backward compat: additive tool (`ci_watch`), additive table, additive notice kind/status (`needs_attention` arm is additive — other kinds unchanged); existing `JobList`/session-event/`job`-tool behavior untouched (all prior suites green).
- Config fallback: no `CODEGG_GITHUB_API_ALLOWLIST` → api.github.com; no token → unauthenticated (public repos within limits; private → 404-ambiguous attention, fail closed). No legacy path to remove.
- Rollback: older builds ignore new table/tool; new watches invisible (documented, acceptable).

## 8. Security review

- Authorization: host-resolved session/turn/principal at creation; exact-owner on status/cancel; stolen handles → `NotFound` (no oracle); unauthorized workdir N/A (no execution); model cannot choose base URL, credential, or owner lineage.
- Secrets: tokens never stored/logged/embedded (rows keep `env:NAME`/`none`); `secret_scan` shared tests pass; Authorization header only to allowlisted origins; request log in tests asserts no credential leakage surface (tool schema has no secret field).
- Path/URL validation: owner/repo charset, 40-hex SHA, selector length, allowlisted https origins (loopback-http only for loopback hosts), userinfo rejected, same-origin redirects ≤3 hops, GHE cross-host traps → attention.
- Privilege boundaries: no execution, no sandbox change, no approval change; cancellation is watch-local; `ShellExec` tool category agreement (observation trigger, not code execution).
- DoS bounds: session quota 8, list cap 32, boundary cap 2, attempt budgets, TTLs, backoff+jitter, Retry-After caps, 256KiB bodies, 500-char summaries, 15s polls; no unbounded provider load; SBC-friendly dormant cost (zero).
- Redaction/audit: notices carry status + validated URL + bounded inert summaries; `permission`/`authorization-matrix` guards green.

## 9. Documentation and operations

- `architecture/jobs.md`: "GitHub Actions watching (M004)" (target, observations, mapping, terminal path, config, limits).
- `architecture/scheduler.md`: "CI watch observation authority (M004)" (why not scheduler jobs / recurring schedules).
- `architecture/authorization.md`: "ci_watch exact-owner" + credential/URL authority.
- Config guidance: `CODEGG_GITHUB_API_ALLOWLIST`, `GITHUB_TOKEN`/`GH_TOKEN` documented in `architecture/jobs.md`; no `codegg.example.jsonc` change (schema keys deferred — untagged-enum validation risk noted as a low finding).
- Tool catalog/discovery: `ci_watch` registered with poll-free guidance (`create/status/cancel/list`); prompt text tells the model background watchers notify on completion.
- Operator diagnostics: bounded `list_for_session` views; `ObserveOutcome` per observation; `delivery_state` per notice; terminal rows carry `conclusion_detail` + `failure_summary`; suppressed/expired leases inspectable; user cancel/steer wins documented.
- Static guards: scheduler-bypass, execution-ownership (unchanged — no new owner), core-boundary green; no `scripts/` changes needed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted GitHub API qualification not performed; evidence is loopback-fixture only | Real api.github.com behavior (pagination shapes, rate-limit headers, Enterprise variants) unproven | A future hosted qualification with a real run id may reference this closure; no rework unless falsified |
| low | Hosted CI (`CI / verify`) / Desktop E2E not run on this SHA | No hosted sweep proof | Same as M001–M003; future hosted run may reference |
| low | Native macOS/Windows not exercised | Platform-specific networking/TLS unproven | Recorded honestly; no platform guarantee claimed |
| low | No live-provider watch trajectory (no model turn consumed a watch notice) | End-to-end model behavior with real inference unproven | Structural gates proven; live trajectory is optional follow-up |
| low | Daemon config keys (not env vars) for allowlist/credentials deferred | Operators configure via `CODEGG_GITHUB_API_ALLOWLIST` env, not `codegg.example.jsonc` keys | Future config-schema pass may promote these to typed keys; validation risk (`codegg validate`) is why keys were not added here |
| low | Fully idle sessions advance watches only on next turn/tool call (no autonomous timer by design) | A watch whose owner never returns and never calls `status` delivers at expiry-attention rather than at remote completion | Documented trade-off of the no-poller design; acceptable per "zero pollers on idle watches" requirement |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed. The async-background-jobs line is now fully closed:

- M001 closed (`e828bebf`), M002 closed (`19028149`), M003 closed (`52d630fc`), M004 closed (`6860aec9`).
- No registered plan lists M004 as a hard dependency; the unblock audit finds nothing newly unblocked. This is recorded explicitly per planning conventions.
- Subsystem roadmap `async-background-jobs-and-ci-watch-roadmap.md` moves to closed (all milestones closed, completion definition met).

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps row: `active (M001–M003 closed; M004 ready)` → `closed` (all milestones closed; closures `001`–`004-status.md`).
- `plans/registry.md` Dependency-ready plans: M004 row `ready` → `closed` (closure `plans/closure/async-background-jobs/004-status.md`, implementation `6860aec9`).
- `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md`: Status `active — M001–M003 closed; M004 ready` → `closed`; M004 `ready` → closed.
- `plans/implementation/async-background-jobs/004-*.md`: `ready for handoff` → `implemented`.
- Unblock audit: nothing newly unblocked — no registered plan depends on M004.
