# Async Background Jobs Corrective C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/async-background-jobs-corrective/002-explicit-idle-owner-continuation.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md#dependency-and-execution-sequence`

Repository baseline reviewed: `8e155498e39118b913c47a7b37a4769223ea1c02`

Implementation commits or pull requests:

- `8e155498` — feat(async-jobs): C002 explicit grant-gated idle owner continuation with exactly-once dispatch

## 1. Executive finding

C002 is complete as a capability + security-invariant slice. An explicitly
authorized user/task request ("run and continue once it finishes") now yields
a real idle-to-active owner turn on the durable terminal event — one newly
accepted canonical turn via the injected executor boundary, without any
further user request or tool call — while default `notify_only` (and any
model-supplied `resume_owner` without a host grant) spends zero tokens and
queues zero turns. The pre-existing M003 next-turn lease marker is preserved
verbatim; legacy leases without grants never auto-resume. No second
scheduler, workflow engine, or unbounded recursion was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| No-actor opt-in: explicit grant + terminal while idle → exactly one canonical owner turn, correct ceilings | `opt_in_grant_dispatches_exactly_one_idle_turn` — grant, ended origin turn, idle `maybe_dispatch` → `Dispatched`, fake executor 1 invocation with owner/session/model/bounded summary; replay → `AlreadyDispatched`, still 1 call | pass | Cause `completed_background_job` persisted before dispatch; turn id deterministic per intent |
| Default: notify_only or missing grant → visible notice, zero provider calls, zero turns, no synthesized prompt | `default_notify_only_or_missing_grant_spends_nothing` | pass | Model `resume_owner` without grant explicitly denied (idle `Suppressed`); M003 marker path untouched |
| CAS/crash: notice→claim→turn-accepted→provider entry; replay never queues two; ambiguous start → needs_attention; 50 contenders + two frontends | `concurrent_contenders_admit_exactly_one_turn` (1 `Dispatched` + 49 `AlreadyDispatched`), `ambiguous_provider_start_becomes_attention_not_replay` (1 call, `NeedsAttention`, no replay) | pass | `UNIQUE(notice_id,grant_id)` + `Pending→Claimed→TurnAccepted→Completed` CAS; `needs_attention` never replayed |
| Security/concurrency: archive, cancel, revocation, expiry, budget, principal change, concurrent turn, wrong workspace/watch/repo, malicious payload, disabled provider fail closed; observer cannot authorize/view | `negative_lineage_cases_all_fail_closed`, `expired_grant_and_exhausted_budget_suppress` | pass | Every mismatch `Suppressed` with notice retained; revocation wins; sandbox/approval ceilings preserved (no bypass) |
| Legacy compatibility: old `resume_owner` leases without grants stay next-turn hints, never idle-dispatch | `legacy_resume_lease_without_grant_stays_notify_only` — M003 `subscribe_with_options(ResumeOwner)` creates `Pending` lease; idle coordinator `Suppressed`, 0 calls | pass | M001/M002/M003/M004, M017, AgentRun M009, foreground/headless/remote dispatch unchanged |
| Canonical routing: same turn-runtime admission, no direct `AgentLoop.run()`, no principal substitution, no unbounded recursion | `IdleContinuationCoordinator` + `ContinuationExecutor` boundary; production routes via injected executor (canonical turn runtime), tests via fake | pass | Scheduler/CI callback never invokes provider directly; budgets + cancel/steer preserved |
| UX/docs: clear `on_complete` semantics, grant/queued/resumed/declined/expired visibility, cancel control, no polling | Tool descriptions (`ci_watch`, `bash`, `test`, `job`), `architecture/{agent,jobs,authorization}.md` | pass | Status via existing session/attention projections; M003 closure preserved as history |

## 3. Production implementation evidence

- **Storage (v77, additive):** `continuation_grant` (grant bound to job/notice,
  session/principal/workspace/origin-turn, allowed model/connection, budgets,
  `max_continuations` default 1 cap 4, `approved` status, expiry) and
  `idle_continuation` (intent `UNIQUE(notice_id,grant_id)`, 9-state machine);
  `STORAGE_LAYOUT_VERSION` 76→77; no backfill (absent grant = notify-only).
- **Authority (`src/scheduler/idle_continuation.rs`, new):**
  `ContinuationService::{authorize, grant_for_job, revoke, intents_for_notice,
  insert/transition/mark}` with DB CAS + in-memory maps;
  `IdleContinuationCoordinator::maybe_dispatch` (resume-gate → resumable-status
  → idle/provider gates → replay short-circuit → usable-grant → lineage bind →
  first-write + `Pending→Claimed` CAS → turn-id + `TurnAccepted` persist →
  executor → `Completed`/`NeedsAttention`); `ContinuationExecutor` trait +
  `FakeContinuationExecutor` (count/capture, failing variant for ambiguity).
- **Preserved:** M003 `JobContinuationLease` creation/consumption and
  next-turn marker text unchanged; `subscribe_with_options` does not reject
  grant-less `resume_owner` (idle path downgrades instead), so all M003 tests
  pass unmodified.
- **Tool/docs:** `on_complete` copy in `ci_watch`/`bash`/`test`/`job` names
  the grant requirement; `architecture/jobs.md` (new C002 section),
  `agent.md` (idle leg), `authorization.md` (`continuation-grant` rule).

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test --test job_continuation_c002_idle_resume
cargo test --test job_continuation_m003_visibility --test job_notification_m001_contract
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

Direct-binary run `./target/debug/deps/job_continuation_c002_idle_resume-* --test-threads=1`
used for deterministic timing after build.

### Results

- `cargo fmt --all -- --check`: pass.
- `job_continuation_c002_idle_resume`: **12/12 pass** (6 secret-scan sentinels
  + 6 C002 tests: opt-in single dispatch, default zero-spend, 50-contender
  exact-once, ambiguous-attention, 8-case negative lineage, expiry/budget,
  legacy non-dispatch). Wall <2s direct-binary.
- M003 `job_continuation_m003_visibility` + M001
  `job_notification_m001_contract`: green, exit 0 (M001 17/17 observed;
  M003 green in same invocation).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
  (`CARGO_BUILD_JOBS=2`): pass after two `new_without_default` findings fixed
  (`Default` for `ContinuationService` + `FakeContinuationExecutor`). ~15m.
- `scripts/verify.sh quick`: pass (fmt, agents check, core/client/desktop
  boundaries, sandbox, ADR integrity, execution-ownership, TUI authorities,
  HTTP disposition, audit, scheduler-bypass, config-merge, provider
  wire/catalog/resilience, OpenAI endpoint, Eggwork routing, skills,
  `cargo check --workspace --all-targets --locked`).
- **Not run (explicit):** hosted `CI / verify` + Desktop E2E on exact head,
  native macOS/Windows, live GitHub, live paid provider. Fake executor only
  per plan; no new permanent CI lane. Hosted qualification deferred to C003.

## 5. Invariant review

- `notify_only` never enqueues model invocation: pass — disposition gate first;
  default and grant-less resume both `Suppressed` with 0 calls.
- Model cannot self-authorize inference: pass — grant requires host-resolved
  ids; model flag alone is denied for idle.
- Durable grant binds principal/session/workspace/task/notice/model/budgets/
  expiry/max-1: pass — `authorize` + lineage re-validation at dispatch.
- Untrusted completion until matched to final state; only compact host refs in
  context: pass — summary is bounded host string; CI titles/logs never enter
  instructions (C001 inert-data rule preserved).
- One admitted turn per notice/grant; no second simultaneous owner loop:
  pass — CAS + `owner_idle` gate; racing cancel/new turn suppresses.
- Crash after provider start → `needs_attention`, no replay; exact-once
  logical admission, no exactly-once-execution claim: pass — failing-executor
  test.
- Same `CoreRequest`/turn-runtime/`JobSubmissionService` path, no direct
  `AgentLoop.run()`, no workflow engine: pass — executor boundary; no
  recursion.
- Fair budgets, negligible idle overhead, cancel/revocation/expiry/quota win:
  pass — gates ordered before dispatch; idle cost is one durable row.

## 6. Failure and recovery review

- Duplicate terminal/replay/second frontend: first-write-wins + CAS; replay
  reports `AlreadyDispatched` without usable-grant requirement (grant consumed
  by winner); 50 contenders → 1 dispatch.
- Crash after notice/claim/turn-accepted: intent states distinguish each
  boundary; `TurnAccepted` persists before dispatch.
- Crash after provider entry (ambiguous): `NeedsAttention`, call counted once,
  retry stays `NeedsAttention` with no second call.
- Cancel/archive/revoke/expiry/budget/principal-change/concurrent-turn/wrong
  lineage/malicious payload/disabled provider: all `Suppressed`, notice
  retained, 0 calls.
- Malformed/unauthorized input: `authorize` fails closed on empty ids;
  over-cap continuations clamped.

## 7. Migration and compatibility review

- Additive v77 only; no M003 table/row mutated; legacy `resume_owner` leases
  keep next-turn semantics and never idle-dispatch.
- `STORAGE_LAYOUT_VERSION` 76→77 tracks highest migration (catalog-invariants
  guard passes via quick).
- Rollback: dropping v77 tables (or downgraded code) leaves C002 grants/
  intents unread (idle stays notify-only); M001/M003/C001 behavior intact.
- Foreground, M001/M002/M003/M004 notices, M017 Tool Programs, AgentRun M009,
  headless/remote dispatch unchanged; no second scheduler/agent runner,
  no high-rate wake loop.

## 8. Security review

- Host consent is the sole autonomy root: `authorize` takes host-resolved
  identity; dispatch re-binds session/principal/workspace; mismatch (including
  newly-appearing principal) fails closed.
- `resume_owner` without grant cannot spend: proven by default test.
- Observer with guessed job/watch id dispatches nothing and views nothing
  (exact-owner rules in C001/M001 preserved; grant lookup is job-scoped).
- Sandbox/approval ceilings, permission broker, secret redaction, URL
  allowlist, and audit behavior unchanged; no credentials in grants/intents;
  secret-scan sentinels pass.

## 9. Documentation and operations

- `architecture/jobs.md`: C002 grant/intent contract, budgets, visibility.
- `architecture/agent.md`: idle leg vs M003 marker, no-auto-spend boundary.
- `architecture/authorization.md`: `continuation-grant` rule.
- Tool `on_complete` copy (`ci_watch`, `bash`, `test`, `job`): grant
  requirement + bounded-token note.
- Operator: grant/revoke via `ContinuationService`; intent states queryable
  via `intents_for_notice`/`get_intent`; user cancel = `revoke` (wins before
  invocation); expired/consumed grants stay auditable rows.
- M003 closure preserved as history; no protocol/migration beyond v77.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted `CI / verify` + Desktop E2E not run on exact head `8e155498` | No hosted qualification for C002 | C003 must obtain exact-head hosted evidence before any merge-ready claim |
| low | Native macOS/Windows, live GitHub, live paid provider not used | Platform/live qualification absent | Labelled unavailable per plan; C003 carries the same condition |
| low | Production executor wiring submits via injected boundary (fake in tests); live provider turn not spent in this environment | Autonomous provider spend proven by fake-executor admission, not by billed inference | C003 exercises the fake-provider coding-turn trajectory; paid live-provider stays out of scope per plan |

No critical/high/medium findings. Stop conditions (model-flag-only dispatch,
unprovable single turn, ambiguous replay) are all discharged — the
implementation proves the single accepted turn via CAS + fake-executor capture.

## 11. Roadmap disposition

Milestone **closed**; hard successor **C003 may proceed to `ready`**.
C003 must validate the real daemon path (C001 idle terminal → C002
grant-gated single turn) rather than a resume marker, using the fake-provider
trajectory and exact-head hosted CI.

## 12. Registry updates

- `plans/registry.md`: move C002 `ready` → `closed` (closure
  `plans/closure/async-background-jobs-corrective/002-status.md`,
  implementation `8e155498`); move C003 `blocked` → `ready` (strict
  C001+C002 closures satisfied).
- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md`:
  status `active (C001 closed; C002 ready; C003 blocked)` → `active (C001 closed; C002 closed; C003 ready)`; milestone table C002 `ready` → `closed`, C003 `blocked on C002` → `ready`.
- `plans/implementation/async-background-jobs-corrective/002-*.md`: `ready for handoff` → `implemented` (done in implementation commit).
- `plans/implementation/async-background-jobs-corrective/003-*.md`: `blocked` → `ready for handoff` (same closure commit per planning unblock rule).
