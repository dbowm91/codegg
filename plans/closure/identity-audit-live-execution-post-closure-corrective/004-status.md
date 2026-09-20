# Identity / Audit Live Execution Corrective M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/identity-audit-live-execution-post-closure-corrective/004-live-execution-audit-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md#M004 — Live execution audit qualification and guard closure`

Repository baseline reviewed: `42b8f9ec44974cd1b70eb95292d5e9b3943e8a24`

Implementation commits:

- `42b8f9ec` — feat(identity): M004 live execution audit qualification and guard closure:
  executor-hook coverage model (`EXECUTOR_LIVE_AUDIT_HOOKS` +
  `FUTURE_DISTRIBUTED_AUDIT_ACTIONS` in `codegg-core`), declarative
  owner pins (`src/executor_audit_hooks.rs`), coverage-guard M004 checks
  + `--self-test`, deterministic qualification trajectory
  (`tests/identity_live_execution_audit.rs`, 4 tests),
  `architecture/audit.md` live/quad-category update, stale
  `check_audit_invariants` path repair (policy + daemon_ops/family).

## 1. Executive finding

M004 is closed. The three corrected single-host actions carry executable
live-owner evidence that cannot regress to builder-only status:
`command_execute` at the canonical tool dispatch, `git_operation` at
`GitMutationExecutor`, `job_complete` at the scheduler terminal
transition. The coverage guard distinguishes daemon request mappings,
executor-owned live hooks, future/distributed actions, and
content-free/high-volume domains, and fails if any live hook is removed.
One deterministic trajectory proves ordered/correlated structural events
with trusted actor, decision ids, project/session/turn/run/job linkage
where available, no bodies/secrets, and no duplicates after
replay/restart. Secret/unauthorized negatives remain green. The original
M005 low single-host live-hook findings are closed. No new high/medium/
low single-host execution-audit finding remains. No ADR was required.
Distributed `node_enrollment`/`remote_execute` remain future scope.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Distinguish daemon mappings, executor live hooks, future/distributed, content-free/high-volume (§2) | `EXECUTOR_LIVE_AUDIT_HOOKS` (3), `FUTURE_DISTRIBUTED_AUDIT_ACTIONS` (2), `INSTRUMENTED` vs `UNINSTRUMENTED` in `audit_instrumentation.rs`; `executor_hook_table_distinguishes_live_future_and_daemon_categories` (core) | pass | Four categories pinned at type level. |
| Do not satisfy guard via `UNINSTRUMENTED_OPERATIONS` names; three actions must have live owner (§2) | Core test asserts executor actions never in `UNINSTRUMENTED`; guard `check_executor_hook_table_is_authoritative` fails if any executor action hides there | pass | Evasion closed. |
| Small declarative executor-hook table consumed by tests/guard; source checks pinned with self-tests, fail closed on move (§2) | `src/executor_audit_hooks.rs` (`EXECUTOR_HOOK_OWNER_PINS`, 9 pins); guard parses the table (`_owner_pins`); `--self-test` proves parsing fails closed; Rust pin test `owner_pins_cover_exactly_the_executor_table` | pass | No ad-hoc call-text grep. |
| Deterministic trajectory: LocalOwner/member turn/job → ToolBroker command → Git mutation → interactive create → scheduler terminal → Owner query (§3) | `single_host_trajectory_is_ordered_correlated_and_secret_free` (ToolBroker bash `echo m004-trajectory`, Git stage, interactive `cat` create, scheduler success, daemon Owner query) | pass | One correlation, seq-ordered. |
| Assert ordered/correlated events, trusted actor, decision ids, project/session/turn/run/job linkage, no bodies/secrets, no duplicates after restart/replay (§3) | Same test: actions `[command_execute, git_operation, command_execute(interactive), job_complete]`, same actor/decision/correlation, session/turn/run/job locators, `assert_structural` on every row, replay + `recover_generation` still 4 rows | pass | Job terminal workspace-scoped (no project) documented where-available. |
| Negatives: unauthorized actor, credential Git URL, secret command, terminal input; secrets absent from metadata/body/error strings (§3) | `unauthorized_project_actor_is_denied_without_execution_leak` (viewer denied); `credential_git_url_secret_command_and_terminal_input_stay_structural` (loopback credential fetch, `ghp_` echo, base64 keystrokes) | pass | Per-row `assert_structural` + secret census. |
| Docs: mark command/git/job live, interactive-create treatment, remove M005 deferred notes, keep node/remote future (§4) | `architecture/audit.md` quad-category cover, live-executor list, interactive-create paragraph, M004 qualification section, verification commands | pass | Historical closures untouched. |
| Skills/AGENTS only where ownership pointers change (§4) | No skill pins the old `daemon:*` owners; no skill change required (recorded here) | pass | `AGENTS.md` rule satisfied. |

## 3. Production implementation evidence

- `crates/codegg-core/src/audit_instrumentation.rs`:
  `command_execute`/`git_operation` → `owner executor:*`, `live_mapped: true`;
  `job_complete` → `owner executor:scheduler` (retry-request mapping
  retained); new `ExecutorAuditHook`, `EXECUTOR_LIVE_AUDIT_HOOKS` (3),
  `FUTURE_DISTRIBUTED_AUDIT_ACTIONS` (2), `is_executor_live_action`/
  `is_future_distributed_action`, plus
  `executor_hook_table_distinguishes_live_future_and_daemon_categories` test.
- `src/executor_audit_hooks.rs` (new): `ExecutorHookPin`,
  `EXECUTOR_HOOK_OWNER_PINS` (9 pins: live-audit + bash/terminal/test/
  interactive + git executor/raw + scheduler terminal/scheduler), plus
  `PINNED_ACTIONS` and table-agreement test.
- `src/lib.rs`: `pub mod executor_audit_hooks`.
- `scripts/check_audit_coverage.py`: `_coverage_actions` anchored on
  `REQUIRED_AUDIT_COVERAGE` (executor table no longer double-counted);
  `check_live_mapped...` exempts executor-live actions;
  `check_executor_hook_table_is_authoritative` (exact triple, live flags,
  no `UNINSTRUMENTED` evasion, command/git executor-only, job_retry
  retained, future unmapped, interactive ops stay uninstrumented);
  `check_m004_executor_owner_pins_present` (parses declarative pins,
  fails closed on move); `--self-test` (parsing + current-tree pass);
  checks 8/8 → 10/10.
- `scripts/check_audit_invariants.py`: stale-path repair required by M004
  verification — canonical `authorization/policy.rs` for operation
  inventory plus `daemon_ops.rs`/`daemon_family.rs` for dispatch. No
  audit semantics changed; guard now 6/6.
- `architecture/audit.md`: quad-category coverage, live-executor list,
  interactive-create treatment, M004 qualification section, verification
  commands extended with M002/M003/M004 matrices + guard self-test.
- `tests/identity_live_execution_audit.rs` (new, 4 tests): table-authority
  pin, full trajectory, unauthorized-actor negative, secret/input
  negatives.

Distinguished as absent by design (not defects):

- No new `AuditAction`, store, schema, migration, protocol bump, or SDK API.
- `command_execute`/`git_operation` have no daemon operation mapping;
  `job_complete` keeps `job_retry` only as the retry request.
- Job terminals stay workspace-scoped (no project locator); Owner
  project query returns the 3 project rows, the terminal joins via the
  store correlation chain.
- Distributed `node_enrollment`/`remote_execute` remain future scope.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_audit_coverage.py --self-test
python3 scripts/check_audit_invariants.py --verbose
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_git_forbidden_patterns.py
cargo test --test identity_m005_audit_instrumentation -- --test-threads=1
cargo test --test identity_m002_live_audit_hooks -- --test-threads=1
cargo test --test identity_m003_scheduler_job_complete -- --test-threads=1
cargo test --test identity_live_execution_audit -- --test-threads=1
cargo test -p codegg-core --lib audit_instrumentation -- --test-threads=1
cargo test -p codegg --lib executor_audit_hooks -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

### Results

- `check_audit_coverage --verbose`: pass, 10/10 (8 pre-existing plus 2 M004).
- `check_audit_coverage --self-test`: pass
  (`audit-coverage self-test ok (executor/future/pin parsing fail closed)`).
- `check_audit_invariants --verbose`: pass, 6/6 (after stale-path repair).
- `check_authorization_matrix --verbose`: pass, 5/5.
- `check_execution_ownership.py`: pass (`execution-ownership guard ok`).
- `check_scheduler_bypass.py`: pass (`scheduler-bypass guard ok`).
- `check_git_forbidden_patterns.py`: pass (0 findings).
- `identity_m005_audit_instrumentation`: pass, 13 passed.
- `identity_m002_live_audit_hooks`: pass, 17 passed.
- `identity_m003_scheduler_job_complete`: pass, 9 passed.
- `identity_live_execution_audit`: pass, 4 passed (table authority;
  trajectory ordered/correlated/secret-free; unauthorized denial;
  credential/secret/input negatives).
- `codegg-core --lib audit_instrumentation`: pass, 17 passed (incl. new
  executor-table test).
- `codegg --lib executor_audit_hooks`: pass, 1 passed.
- `cargo test --workspace --locked -- --test-threads=1`: pass (no FAILED).
- `scripts/verify.sh quick`: pass (`==> Quick verification passed.`).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass,
  0 warnings.
- `git diff --check`: pass.

Guard negative proof: temporarily renaming `emit_command_execute` in
`src/live_execution_audit.rs` makes both the M002 hook check and the M004
owner-pin check FAIL (`missing live-hook marker`, `missing hook marker`);
restoring the symbol returns 10/10. Removing any of the three actions
from `EXECUTOR_LIVE_AUDIT_HOOKS` breaks the authoritative-table check and
the core `executor_hook_table_...` test.

End-to-end event chain (one correlation `corr-m004-trajectory`):

| Seq order | Action | Family/outcome | Locators |
|---|---|---|---|
| 1 | `command_execute` | `shell` / `success` | project/session/turn/run/job + digest |
| 2 | `git_operation` | `stage` / `completed` | project/session/turn/run/job + ref digest |
| 3 | `command_execute` | `interactive` / `success` | project/session/turn/run/job + argv digest |
| 4 | `job_complete` | `success` | session/turn/run/job, no project (workspace scope) |

Replay + `recover_generation` over the terminal job leaves the count at 4.

Secret census: every trajectory/negative row has `body_ref=None`,
`content_digest=None`, bounded digest/label metadata only. Forbidden
needles (`m004-trajectory` command text, `m004.txt`, `ghp_` secret,
`s3cret-m004`, `127.0.0.1`, `keystrokes`, URL userinfo) absent from all
metadata blobs. Store secret guard never trips because secrets never
enter metadata preimages as stored values (digests only).

All evidence above is local execution and is labeled accordingly. No
hosted `CI / verify` run is attached; routine CI remains the operator
action per the closed development-verification roadmap.

## 5. Invariant review

- Actor/provenance from trusted transport/daemon state only: trajectory
  context built from the Owner principal + team-membership provenance;
  broker threads it without synthesis; scheduler terminal rebuilds from
  the durable attribution row (same decision/correlation); legacy
  fallback untouched.
- Coordinator remains sequence/store authority: all hooks emit through
  `ExecutionAuditEmitter` (bounded 500 ms, counters/warn); deterministic
  ids dedupe via `ON CONFLICT DO NOTHING`; no second store/queue/bus.
- No content in audit state: digests hex, labels bounded; URL scrub
  before git digests; terminal input/cwd/env never enter metadata;
  per-row structural negatives green.
- One transition → one event: broker never emits; Git route silences
  shell; interactive lifecycle beyond create silent; terminal owners emit
  once after durable acceptance; replays dedupe (tested).
- Bounded best-effort failure: emitter policy unchanged (M001); audit
  failure never fails execution.
- Execution ownership not bypassed: bash still routes via intent;
  Git via `GitMutationExecutor`; scheduler via terminal writes;
  execution-ownership + scheduler-bypass guards pass;
  `docs/execution-ownership.toml` unchanged (no new spawn surface).
- Personal-local and team share one composition: same context/emitter/
  hook types; trajectory uses team Owner, negatives use viewer denial.

## 6. Failure and recovery review

- Pool-less emitter drops with `dropped_no_pool` (M001-tested, unchanged).
- Store failure/timeout increments `failed` with warn; owning
  command/git/interactive/scheduler transitions complete on their own
  terms.
- Retry/replay: broker `submission_key` reuses the command id; git scope
  reuses the event id; scheduler replay fails `InvalidTransition` before
  emit; `recover_generation` over terminal jobs emits nothing (tested).
- Cancellation races: timeout/cancelled/interrupted vocabularies
  unchanged (M002/M003-tested); trajectory uses success only.
- Malformed/unauthorized: viewer query denied at the gate with an
  attributable `authorization_decision` terminal; denied tool/git paths
  emit no execution event (M002-tested, unchanged).

## 7. Migration and compatibility review

- No storage migration, no schema change, no catalog layout change.
- No protocol change, no `PROTOCOL_VERSION` bump; new contexts/hooks
  have no `serde` impls and never appear in DTOs.
- Backward compatible: all new core APIs additive; new consts/tables
  read-only; existing suites green.
- Rollback: dropping `42b8f9ec` restores silent-hook-tolerant guards at
  M003 level; rows already written remain ordinary queryable audit rows.

## 8. Security review

- No new authority: emitters decide nothing; hooks fire only after the
  owning allow/admission/terminal decision.
- No principal fabrication: contexts from daemon-owned state or durable
  attribution; `reconstructed` unreachable from wire input.
- No wire injection: no `serde` on contexts/hooks/emitters/pins.
- Secrets: digests/labels only; credential URLs scrubbed before hashing;
  secret-bearing command/input material never stored; error strings from
  failed fetch/echo never enter audit metadata (asserted).
- Denial-of-service bounds: one event per executed/terminal transition,
  no unbounded queues, no new network/spawn surface; counters expose
  pressure.
- `codegg-core` boundary guard passes via `verify.sh quick`.

## 9. Documentation and operations

Updated:

- `architecture/audit.md` — quad-category coverage, live-executor list,
  interactive-create treatment, M004 qualification section, extended
  verification commands.
- `scripts/check_audit_coverage.py` — M004 authoritative-table + owner-pin
  checks + `--self-test` (10/10).
- `scripts/check_audit_invariants.py` — stale-path repair (policy +
  daemon_ops/family); 6/6.
- `src/executor_audit_hooks.rs` — declarative owner pins + agreement test.
- `plans/implementation/identity-audit-live-execution-post-closure-corrective/004-live-execution-audit-qualification.md`
  — marked closed, linking this record.
- `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md`
  — M004 `ready` → `closed`; workstream `active` → `closed`.
- `plans/registry.md` — subsystem row, dependency-ready table,
  execution-order gate (see §12).

Operator note: no migration, no config change. `appended` grows by up to
3 rows per audited trajectory (command + git + interactive) plus one per
scheduler terminal. `failed` growth still means audit I/O saturation.

## 10. Unresolved findings

There are no unresolved critical, high, medium, or low M004 findings.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | — | — | — |

Explicit non-claims with named consumers (not defects):

- Distributed `node_enrollment`/`remote_execute` remain future scope;
  builders landed, no live emission.
- Job terminals remain workspace-scoped (no project locator);
  correlation/session/turn/run/job linkage is the cross-query key.
- Hosted `CI / verify` remains the operator action; all evidence here is
  local and labeled as such.

No stop condition fired: audit authority stays with the daemon/
coordinator; no second store/bus; no trusted principal in DTOs; no
authorization-model change.

## 11. Roadmap disposition

Milestone closed and workstream complete:

- M004 `ready` → `closed`.
- Identity/audit live-execution post-closure corrective `active` →
  `closed`: M001+M002+M003+M004 all closed. The original M005 low
  single-host live-hook findings are closed. Distributed node/remote
  audit remains explicitly future scope and does not reopen this
  workstream.
- No corrective pass is required.

## 12. Registry updates

Included in the closure commit alongside this record:

- M004 source plan marked closed, linking this record.
- Roadmap milestone table: M004 `ready` → `closed` with closure link;
  roadmap `Status: active` → `Status: closed`.
- Registry active-subsystem row: Identity `active / M001+M002+M003
  closed; M004 ready` → `closed / M001+M002+M003+M004 closed`;
  blocker column records campaign completion with distributed scope
  remaining future.
- Registry dependency-ready table: M004 row `ready` → `closed` with
  closure link and implementation `42b8f9ec`.
- Registry execution-order gate: post-closure cleanup gate advanced from
  `M001+M002+M003 closed; M004 ready` to `M001+M002+M003+M004 closed`.
- Registry blocked-work audit: no registered plan lists M004 as a hard
  or interface dependency; no plan moves from `blocked`/`proposed` to
  `ready` in this commit. Deferred unregistered product work (remote
  node/distributed execution) remains intentionally unregistered.
- No new corrective or follow-up plan registered; the workstream is
  complete.
