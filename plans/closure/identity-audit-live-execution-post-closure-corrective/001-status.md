# Identity / Audit Live Execution Corrective M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/identity-audit-live-execution-post-closure-corrective/001-trusted-execution-audit-context-and-emitter.md`

Source subsystem roadmap:

- `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md#M001 — Trusted execution audit context and emitter seam`

Repository baseline reviewed: `7c75c00933700a82ab836885270fc5ac823fe0bf`

Implementation commits:

- `7c75c009` — feat(identity): M001 trusted execution audit context and emitter:
  `TrustedExecutionAuditContext` + `ExecutionAuditEmitter` in
  `codegg-core::audit_instrumentation`, daemon `audit_emitter` /
  `execution_audit_context` seam with `append_audit_event` delegation,
  `BrokerInvocationContext` / `ToolExecutionContext` preservation,
  `GitMutationExecutor` and `JobScheduler` injection points, seam unit
  tests, coverage-guard pins, `architecture/audit.md` (+ tool/scheduler/
  Git docs).

## 1. Executive finding

M001 is closed. One daemon-owned cloneable bounded emitter plus one
trusted execution-audit context now exist, and canonical execution
owners receive attribution by injection without fabricating actor
identity or owning the audit store. No new event, store, schema, or
protocol was added. M002 and M003 can be implemented against this seam
without designing a second audit mechanism; no ADR was required.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One internal cloneable emission service with bounded timeout, existing counters/warn, no separate queue/store/schema, no auth decisions | `ExecutionAuditEmitter::new(pool)` + `EXECUTION_AUDIT_EMIT_TIMEOUT` (500 ms) + `emit`/`emit_with` in `crates/codegg-core/src/audit_instrumentation.rs`; `CoreDaemon::audit_emitter` + `append_audit_event` delegation in `src/core/daemon.rs` | pass | Daemon family handlers keep calling `append_audit_event`; injected owners call `emit`/`emit_with` with the same policy. |
| Trusted execution-audit context with principal/provenance/chain only, no command/body/secret | `TrustedExecutionAuditContext::new` + `legacy_local` + `principal`/`provenance`/`chain` + `builder` + `child_with_parent` in `audit_instrumentation.rs`; no `serde` impls | pass | Cloned immutable transport-bound principal, gate-copied provenance, chain locators only. |
| Narrowest dependency-safe layer, core boundary preserved | Types live in `codegg-core::audit_instrumentation` (imports only `audit`, `identity`, `transport_auth`, `sqlx`, `tokio`, `tracing`); `bash scripts/check-core-boundary.sh` pass; `python3 scripts/check_audit_coverage.py` production-boundary pin pass | pass | No `crate::authorization` import in production instrumentation code. |
| Thread trusted origin through ToolBroker without synthesis from `principal_ref`/input/model/grant | `BrokerInvocationContext::execution_audit` + `with_execution_audit` + `From<ToolExecutionContext>` preservation in `src/tool/broker.rs`; `ToolExecutionContext::execution_audit` + `apply_execution_audit` in `src/tool/backend.rs`; `execute_with_retry` projects origin strings only from the trusted context; agent-loop and scheduler-program paths thread `None`/clone explicitly | pass | `principal_ref`-only contexts stay `None`; broker tests pin both preservation and no-synthesis. |
| Additive optional context for legacy/local callers with explicit `legacy_local`/LocalOwner provenance | `TrustedExecutionAuditContext::legacy_local` (LocalOwner binding, `legacy-local` decision, `local_owner_broad` policy); broker/executor fields are `Option` with `None` default | pass | No invented human identity; legacy rows remain explicitly marked. |
| Seam unit tests: principal preservation, correlation, no-secret fields, timeout/failure counters, no duplicate stores | 7 new core tests + 2 new broker tests (see §4) | pass | 16/16 core `audit_instrumentation` tests; 6/6 broker tests. |
| Docs: `architecture/audit.md` + core/tool/scheduler skills/docs | `architecture/audit.md` "Trusted execution audit seam"; `architecture/tool_broker.md`, `architecture/scheduler.md`, `architecture/git.md` M001 notes; `scripts/check_audit_coverage.py` M001 seam pins | pass | Source plan required audit.md plus relevant core/tool/scheduler docs; skills carry no stale ownership pointer requiring edits. |
| Non-goals held: no `command_execute`/`git_operation`/`job_complete` emission, no wire/storage migration, no public SDK, no TeamStore/authorization queries from execution layers | `git status` shows no migration, no protocol change; scheduler/Git fields are injection-only with no emit call; M002/M003 plans unchanged except status | pass | M001 emits only in tests via `emit_with`. |

## 3. Production implementation evidence

- `crates/codegg-core/src/audit_instrumentation.rs` (+~230 lines):
  `EXECUTION_AUDIT_EMIT_TIMEOUT`, `TrustedExecutionAuditContext`
  (new/legacy_local/getters/builder/child), `ExecutionAuditEmitter`
  (new/with_timeout/emit/emit_with, pool-optional, timeout + counters +
  warn), 7 focused tests. Boundary-clean: no `crate::authorization`
  import in production code.
- `src/core/daemon.rs`: `audit_emitter()` builds the shared emitter
  from `self.pool`; `execution_audit_context(authority, decision,
  request)` copies principal/provenance/chain; `append_audit_event`
  delegates to the emitter (one policy for family handlers and
  injected owners).
- `src/tool/backend.rs`: `ToolExecutionContext::execution_audit`
  (`Option<TrustedExecutionAuditContext>`), `apply_execution_audit`
  (clones context + projects trusted origin strings), `execution_audit`
  accessor; `with_backend` defaults to `None`.
- `src/tool/broker.rs`: `BrokerInvocationContext::execution_audit` +
  `with_execution_audit` + accessor; `From<ToolExecutionContext>`
  preserves it; `execute_with_retry` clones it into `exec_ctx` via
  `apply_execution_audit` (origin strings projected only from trusted
  context). Agent-loop (`src/agent/tool_batch.rs`) and scheduler
  program (`src/scheduler/tool_program_executor.rs`) construction sites
  updated; 2 new preservation/no-synthesis tests.
- `src/git_mutations.rs`: `GitMutationExecutor` gains optional
  `execution_audit` + `audit_emitter` with
  `with_execution_audit`/`with_audit_emitter` and accessor; no emission
  in M001 (M002 consumer).
- `src/scheduler/scheduler.rs`: `JobScheduler` gains
  `audit_emitter: Arc<AsyncMutex<Option<ExecutionAuditEmitter>>>` with
  `set_audit_emitter` / `audit_emitter_snapshot`; no emission in M001
  (M003 consumer).
- Test-only construction churn: 12 test files add
  `execution_audit: None` to `BrokerInvocationContext` literals; 4 test
  files add it to `ToolExecutionContext` literals. No behavior change.
- `scripts/check_audit_coverage.py`: pins `TrustedExecutionAuditContext`,
  `ExecutionAuditEmitter`, `EXECUTION_AUDIT_EMIT_TIMEOUT` in the
  instrumentation module, `audit_emitter` + `execution_audit_context`
  in the daemon seam, and `execution_audit`/`audit_emitter` markers in
  broker/backend/Git/scheduler owners.
- Docs: `architecture/audit.md` M001 seam section;
  `architecture/tool_broker.md`, `architecture/scheduler.md`,
  `architecture/git.md` ownership notes.

Distinguished as absent by design (not defects):

- No live `command_execute` / `git_operation` / `job_complete` hooks;
  M002/M003 own them.
- No storage migration, no protocol change, no public SDK API.
- Scheduler/Git hold the seam but do not emit yet.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib audit_instrumentation -- --test-threads=1
cargo test --test identity_m005_audit_instrumentation -- --test-threads=1
cargo test -p codegg --lib tool::broker -- --test-threads=1
cargo test -p codegg --lib scheduler -- --test-threads=1
python3 scripts/check_audit_coverage.py --verbose
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

### Results

- `cargo test -p codegg-core --lib audit_instrumentation`: pass, 16
  passed (9 pre-existing + 7 new M001: preservation, child
  correlation, legacy_local provenance, no-secret shape, no-pool drop
  counter, shared-policy append + single-store query, failure counter).
- `cargo test --test identity_m005_audit_instrumentation`: pass, 13
  passed (no M005 regression).
- `cargo test -p codegg --lib tool::broker`: pass, 6 passed (4
  pre-existing + 2 new M001: preservation when supplied,
  no-synthesis from `principal_ref`).
- `cargo test -p codegg --lib scheduler`: pass, 78 passed (no
  scheduler regression; new emitter field is additive).
- `python3 scripts/check_audit_coverage.py --verbose`: pass, 6/6
  (including new M001 seam pins).
- `bash scripts/check-core-boundary.sh`: pass.
- `python3 scripts/check_execution_ownership.py`: pass
  (`execution-ownership guard ok`; no new spawn surface).
- `scripts/verify.sh quick`: pass (`==> Quick verification passed.`;
  covers fmt, agent schema, core-boundary, sandbox,
  execution-ownership, tui-authority, http-route-disposition,
  audit-coverage, scheduler-bypass, workspace check).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`:
  pass, 0 warnings.
- `git diff --check`: pass.

All evidence above is local execution and is labeled accordingly. No
hosted `CI / verify` run is attached; routine CI remains the operator
action per the closed development-verification roadmap.

## 5. Invariant review

- Actor/provenance derives from trusted transport/daemon decision,
  never caller/model strings: context holds the cloned
  `AuthenticatedPrincipal` (private fields, trusted constructors only)
  plus gate-copied provenance; broker projects origin strings only
  from the supplied context; `principal_ref` alone yields `None`
  (tested).
- Coordinator remains sequence/store authority: emitter wraps
  `AuditStore::new(pool)` with the daemon timeout/counters; no second
  database, bus, or queue; scheduler/Git receive clones by injection.
- No command/argv/input/URL/file/tool-output content in audit state:
  context has exactly three fields (principal/provenance/chain);
  builders accept digests/labels/ids only; debug-shape test pins the
  absence of payload-shaped fields; store redaction still rejects
  secret-bearing keys/values/bodies.
- One transition emits at most one event: M001 adds no live emission;
  `emit_with` builds one builder per call; deterministic ids remain
  available to M002/M003 for idempotent terminal events.
- Bounded best-effort failure: pool-less emits drop with
  `dropped_no_pool`; timeouts/failures increment `failed` with warn
  logs and never fail the owning operation (tested).
- Execution ownership not bypassed: ToolBroker remains the tool-call
  boundary; scheduler/Git changes are additive injection points;
  execution-ownership and scheduler-bypass guards pass.
- Personal-local and team execution share one composition: same
  context/emitter types; legacy paths use explicit `legacy_local`
  rather than a second mechanism.

## 6. Failure and recovery review

- Pool-less daemon: `ExecutionAuditEmitter::new(None)` drops with
  `record_emit_dropped_no_pool` (tested); matches legacy in-memory
  behavior.
- Store failure / timeout: `failed` counter increments with warn, no
  exception propagates to the owner (failure-counter test uses a
  closed pool with 50 ms timeout).
- Retry/replay: no new live emission, so no duplicate risk in M001;
  `TrustedExecutionAuditContext::builder` applies the stored chain
  (including deterministic event id when present) so M002/M003 replays
  reuse ids.
- Cancellation races: emitter performs one bounded `store.append`
  under `tokio::time::timeout`; no background task or queue to drain
  on shutdown.
- Malformed/unauthorized input: unknown operations still emit nothing
  (`operation_to_audit_action` returns `None`); `principal_ref`-only
  callers receive no trusted context (tested).

## 7. Migration and compatibility review

- No storage migration: no schema change; catalog layout unchanged.
- No protocol change: no new `CoreRequest`/`CoreResponse` variants, no
  `PROTOCOL_VERSION` bump; `TrustedExecutionAuditContext` has no
  `serde` impls and never appears in DTOs.
- Backward compatible: all new fields are `Option` with `None`
  defaults; existing `BrokerInvocationContext`/`ToolExecutionContext`/
  `GitMutationExecutor`/`JobScheduler` construction sites compile with
  additive `None`/setter updates only.
- Rollback: dropping the M001 commit restores prior daemon-direct
  appends; no durable state depends on the new types.

## 8. Security review

- No new authority: emitter performs no authorization decisions;
  context construction requires the already-admitted
  principal/decision pair at the daemon boundary.
- No principal fabrication: `AuthenticatedPrincipal` fields are
  private; the only new constructor taking strings is `legacy_local`,
  which records the explicit local-owner marker rather than a team
  identity.
- No wire injection: without `Serialize`/`Deserialize`, JSON tool
  inputs, model outputs, and request DTOs cannot supply the context.
- Secrets: context carries no payload fields; builders still reject
  secret-bearing metadata/bodies with `audit_secret_detected` before
  any write (existing M005 secret-negative suite still green).
- Denial-of-service bounds: one 500 ms per-write timeout, no unbounded
  queue, no new network/spawn surface; counters expose pressure.
- `codegg-core` boundary guard passes; scheduler/Git/broker remain in
  the root crate and consume core types only.

## 9. Documentation and operations

Updated:

- `architecture/audit.md` — "Trusted execution audit seam (M001)":
  emitter policy, context contents, daemon constructors, broker/
  scheduler/Git threading, legacy rule, M002/M003 consumer note.
- `architecture/tool_broker.md` — trusted-audit preservation invariant.
- `architecture/scheduler.md` — `audit_emitter` field + setter methods.
- `architecture/git.md` — executor injection-point note.
- `scripts/check_audit_coverage.py` — executable M001 seam pins (6/6).
- `plans/implementation/identity-audit-live-execution-post-closure-corrective/001-trusted-execution-audit-context-and-emitter.md`
  — marked closed, linking this record.
- `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md`
  — M001 `ready` → `closed`; M002/M003 `blocked` → `ready`.
- `plans/registry.md` — subsystem row, dependency-ready table, blocked
  work, execution-order gate, closure-evidence table (see §12).

Operator note: no migration, no config change, no new event volume.
Monitor the existing `emit_counters_snapshot`
(`appended`/`failed`/`dropped_no_pool`); M002/M003 will increase
`appended` as live hooks land. `failed` growth still means audit I/O
saturation — shed load rather than retrying with fresh event ids.

## 10. Unresolved findings

There are no unresolved critical, high, medium, or low M001 findings.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | — | — | — |

Explicit non-claims with named consumers (not defects):

- Live `command_execute` / interactive / `git_operation` hooks remain
  M002 scope against this seam.
- Live scheduler `job_complete` remains M003 scope against this seam.
- Guard/trajectory qualification remains M004 scope after M002+M003.
- Distributed `node_enrollment` / `remote_execute` remain future scope.

No stop condition fired: sequence/store authority stays with the
coordinator; no second audit database or event bus; no trusted
principal/provenance in public untrusted DTOs; no authorization-model
change.

## 11. Roadmap disposition

Milestone closed and next dependencies may proceed:

- M001 `ready` → `closed`.
- M002 (command/interactive/Git live hooks) `blocked` → `ready`: its
  sole hard dependency (M001 seam) is satisfied; interface contracts
  (`TrustedExecutionAuditContext`, `ExecutionAuditEmitter`,
  broker/Git threading) are stable.
- M003 (scheduler `job_complete`) `blocked` → `ready`: same sole hard
  dependency satisfied; scheduler injection point is stable.
- M004 remains `blocked` on M002+M003 closure (unchanged).

No corrective pass is required.

## 12. Registry updates

Included in the closure commit alongside this record:

- M001 source plan marked closed, linking this record.
- Roadmap milestone table: M001 `ready` → `closed` with closure link;
  M002/M003 `blocked` → `ready`; M004 stays `blocked` on M002+M003.
- Registry active-subsystem row: Identity current milestone `M001
  ready; M002+M003 blocked; M004 blocked` → `M001 closed; M002+M003
  ready; M004 blocked`; blocker column narrowed to the remaining
  hook/qualification dependencies.
- Registry dependency-ready table: M001 row `ready` → `closed` with
  closure link; M002 and M003 rows added as `ready` with hard
  dependency noted satisfied.
- Registry execution-order gate: post-closure cleanup gate advanced
  from "M001 ready; M002/M003 blocked; M004 blocked" to "M001 closed;
  M002+M003 ready; M004 blocked on M002+M003".
- Registry blocked-work table: M002/M003 unblocked and removed from
  the combined blocked row; M004 retained as blocked on M002+M003.
- Registry closure-evidence table: Identity M001 row added pointing at
  this record with implementation `7c75c009`.
