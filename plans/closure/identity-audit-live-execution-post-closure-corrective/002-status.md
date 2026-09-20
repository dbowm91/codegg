# Identity / Audit Live Execution Corrective M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/identity-audit-live-execution-post-closure-corrective/002-command-interactive-git-live-audit-hooks.md`

Source subsystem roadmap:

- `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md#M002 — Command, interactive-process, and Git live audit hooks`

Repository baseline reviewed: `e0cc7f05`

Implementation commits:

- `e0cc7f05` — feat(identity): M002 command/interactive/Git live audit
  hooks: `command_execute` at the narrowest per-family dispatch owners
  (bash shell/test/process, terminal process, scheduler-owned test,
  interactive create) plus `git_operation` at `GitMutationExecutor`
  (typed, network, recovery, raw paths) through the M001 trusted
  context/emitter seam; structural digests/labels only, deterministic
  event identity for retries, M002 coverage-guard pins,
  `architecture/audit.md` (+ git doc), 17-test regression matrix
  `tests/identity_m002_live_audit_hooks.rs`.

## 1. Executive finding

M002 is closed. Real command/process dispatches emit one structural
`command_execute` event with trusted attribution and no command text;
interactive process creation emits one structural event without
terminal input retention; mutating/network/recovery Git operations
emit one `git_operation` event with no secrets; native Git and
bash-routed Git converge on one audit event, not duplicates. Denied
and pre-dispatch failures emit no execution event. Existing
execution/security behavior is unchanged apart from the new
structural audit records (all hooks are silent without the threaded
M001 context/emitter pair). No new AuditAction, store, schema, or
protocol was added. No ADR was required.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| One `command_execute` terminal event for real process/command execution on the `ToolBroker::execute/execute_with_retry` path, never for read-only tools (§2) | `command_family_for_tool` ownership matrix in `src/live_execution_audit.rs` (bash→shell, terminal→process, test→test, all others→None); broker itself never emits; `command_execute_ownership_matrix` test | pass | Narrowest-owner selection per family, tested in one place. |
| Digest of normalized command/argv bytes, never argv text; bounded family label (§2) | `command_digest_argv` (NUL-joined) / `command_digest_str` (SHA-256 hex); families `shell`/`test`/`process`/`interactive`; metadata-negative tests assert no command text in any row | pass | Boundary-collision unit test pins the NUL join. |
| Project/session/turn/run/job causation preserved from M001 context (§2) | `emit_command_execute` builds from `TrustedExecutionAuditContext` chain; tests assert session/turn/run locators on stored rows | pass | Chain applied losslessly via `emit_with`. |
| No successful event when dispatch never occurs (§2) | Denied/pre-dispatch paths return before any emit: bash blocked commands, terminal/test/git permission and admission errors, `Rejected` routes; `bash_failure_timeout_and_denial_matrix`, `test_tool_without_scheduler_emits_nothing`, `denied_git_mutation_emits_no_execution_event` | pass | Denials remain audited as `authorization_decision` elsewhere. |
| Uncertain/timeout/cancel distinguishable without invented success (§2) | Outcome vocabulary `success`/`failure`/`timeout`/`cancelled`/`uncertain`; live tests for success, failure, timeout (bash/terminal), cancelled + uncertain (scheduler test executor), timeout (bash observe); timeout arms emit before propagating the error | pass | Cancel substitution recorded in §4 (no `interactive_process_m001` target; broker-level cancellation covered by outcome mapping, live cancel via scheduler executor). |
| Retries of one logical invocation reuse deterministic identity (§2) | `invocation_scope` (key + outcome; digest fallback for legacy callers); `bash_retry_with_same_invocation_key_does_not_duplicate` (replay→1 row, distinct key→2 rows); `deterministic_event_id` reuse via store `ON CONFLICT DO NOTHING` | pass | Distinct outcomes scope separately so a timeout then a success stay two truthful rows. |
| No duplicate command events across ToolBroker/Bash/managed-process layers (§2) | Broker holds no emitter and emits nothing; bash emits only for non-Git executors (`audit_shape_for_executor` returns `None` for `Git`/`Rejected`); `executor_shapes_route_git_to_silence` + convergence tests | pass | One canonical owner per family; matrix tested. |
| Interactive create emits `command_execute` on dispatch with transport-bound principal/provenance, no input bodies (§3) | `InteractiveAuditHook` + `emit_interactive_create_audit` in `src/interactive_process_attach.rs`; daemon builds the hook in `CoreDaemon::interactive_audit_hook` (registry-bound principal, daemon-asserted `interactive_transport` provenance, envelope correlation); `interactive_create_emits_without_input_retention` (create→1 row; attach/input/resize/detach/list→still 1 row; input-secret negative) | pass | Terminate/remove emit nothing per plan (no new AuditAction invented). |
| `git_operation` exactly once for mutation/network/recovery transitions with bounded label + ref digest + terminal outcome (§4) | `GitMutationExecutor::emit_git_operation` at the end of `execute` (covers typed, network via `git_network_ops`, recovery via `run_recovery`) + `run_raw_mutation` hook + tool raw-fallback `emit_raw_git_audit`; labels via `git_audit_op_label` (`stage`/`commit`/`branch_create`/`merge`/`rebase`/`push`/`pull`/`fetch`/`recover_*`/…, read-only→None); outcome = terminal `MutationOutcome` label | pass | Recovery reclassification happens after `execute`, so the audited outcome is the executor-classified terminal (completed/conflict/…); truthful and bounded. |
| No URLs with credentials, messages, paths, patches, or output in git audit (§4) | `git_audit_ref_digest` digests remote NAMES/refs/revs only; URLs never enter even the preimage (URL scrub defense-in-depth); commit messages/paths hash empty; `ref_digest_covers_names_and_never_urls`, `managed_argv_digest_excludes_raw_argv`, network credential-negative integration test | pass | `RedactedUrl::expose_secret` untouched; execution still uses the raw URL. |
| Read-only status/diff/log/blame need no `git_operation` (§4) | `git_audit_op_label` returns `None` for all read-only variants; `read_only_operations_emit_no_git_event` + `read_only_git_operations_emit_nothing` (live status→0 rows) | pass | No policy change. |
| Full regression matrix (§5) | `tests/identity_m002_live_audit_hooks.rs` (17 tests) + unit suites below | pass | See §4. |

## 3. Production implementation evidence

- `src/live_execution_audit.rs` (new, ~230 lines): `OUTCOME_*`
  vocabulary, `command_family_for_tool` ownership matrix,
  `command_digest_argv`/`command_digest_str`, `invocation_scope`,
  `emit_command_execute`, `audit_shape_for_executor`
  (Git/Rejected→`None`), `planned_audit_shape` (Git-route timeouts
  described once as shell timeouts since the executor emits nothing
  on its error paths), 5 unit tests.
- `src/tool/backend.rs`: `ToolExecutionContext.audit_emitter`
  (`Option<ExecutionAuditEmitter>`) + `apply_audit_emitter` +
  `audit_emitter` + `live_audit_hook` (emits only when context AND
  emitter are both present).
- `src/tool/broker.rs`: `BrokerInvocationContext.audit_emitter` +
  `with_audit_emitter`/accessor, `From<ToolExecutionContext>`
  preservation, per-attempt propagation into `exec_ctx`
  (mirrors M001); broker emits nothing itself. 1 new threading test
  (7/7 broker tests green).
- `src/tool/bash.rs` + `src/tool/bash/process.rs`: `execute` split
  into `execute_inner(input, audit_ctx)` with an
  `execute_structured` override; `dispatch_command_target` /
  `dispatch_to_git` thread the context into the executor builders;
  active-path success emits via the actual-executor shape (Git
  silent), active/observe timeouts emit `timeout` with the planned
  shape, all other pre-dispatch errors emit nothing.
- `src/tool/terminal.rs`: `execute_inner` + `execute_structured`
  override; family `process`; timeout arm emits before propagating.
- `src/tool/test.rs`: `run_scheduled_test` returns
  `(summary, ExecutorStatus)`; `execute_structured` emits family
  `test` with Completed→success / Failed→failure /
  Cancelled→cancelled / TimedOut→timeout /
  Interrupted→uncertain; no-submission errors emit nothing.
- `src/tool/git.rs`: `execute_inner` + `execute_structured`
  override; `dispatch_mutation`/`dispatch_recover` build the
  executor with the threaded audit pair; raw fallback emits one
  event for non-read-only subcommands with a sanitized token label
  (`unknown` for non-token input) and the empty ref digest, plus a
  timeout arm; child-policy denials return before any emit.
- `src/git_mutations.rs`: `emit_git_operation` (+ shared
  `emit_git_operation_parts`), `git_audit_op_label`,
  `git_audit_ref_digest`; `execute()` hook covers typed, network,
  and recovery transitions (single event per transition; note the
  composed `commit_with_selection` stage+commit flow yields one
  `stage` and one `commit` event — two real transitions, not a
  duplicate). 4 new unit tests.
- `src/git_mutations_ops.rs`: `run_raw_mutation` emits through the
  same executor hook (fetch-`--prune`, `add -A`, etc.).
- `src/interactive_process_attach.rs`: `InteractiveAuditHook` +
  `create(..., audit)` + `emit_interactive_create_audit`
  (argv digest, handle-bound scope, `interactive` family).
- `src/core/daemon.rs`: `interactive_audit_hook` (transport-bound
  principal via `request_authority_for_client`, daemon-asserted
  `interactive_process_create`/`interactive_transport` provenance —
  this path predates the M003 gate so no gate decision exists to
  copy — envelope correlation) threaded through
  `run_interactive_request` into the create arm only.
- `src/agent/tool_batch.rs`, `src/scheduler/tool_program_executor.rs`:
  `audit_emitter` threaded alongside `execution_audit` (both `None`
  in production today, matching M001 dormancy; hooks activate as
  turn/job boundaries supply the pair — M003/M004 scope).
- `src/lib.rs`: `pub mod live_execution_audit`.
- Test-only churn: 15 test files add `audit_emitter: None` to
  context literals; `tests/interactive_process_attach_resume.rs` +
  `tests/interactive_terminal_tui.rs` pass `None` for the new create
  hook (silent harness behavior preserved).
- `scripts/check_audit_coverage.py`: new 7th check
  `check_m002_live_execution_hooks_present` pinning every hook site
  (7/7 green).
- Docs: `architecture/audit.md` "Live execution hooks (M002)" +
  gap-list narrowing (`job_complete` remains the deferred hook);
  `architecture/git.md` executor row updated.

Distinguished as absent by design (not defects):

- No live `job_complete` hook; M003 owns it against the same seam.
- No storage migration, no protocol change, no public SDK API.
- Daemon turn/job boundaries still thread `None` (M001-staged);
  production emission activates when they supply the pair.
- Terminate/remove emit no command event; no new AuditAction.
- Raw-subcommand git mutations without snapshots scope without a
  post-state digest (op + empty ref + outcome still
  deterministic per logical invocation).
- `rerun`-style duplicate tool submissions with the same invocation
  key intentionally reuse the event id (idempotency, not loss).

## 4. Verification executed

### Commands run

```bash
cargo test --test identity_m002_live_audit_hooks -- --test-threads=1
cargo test --test identity_m005_audit_instrumentation -- --test-threads=1
cargo test --test git_execution_origin_matrix -- --test-threads=1
cargo test --test git_credential_cross_path -- --test-threads=1
cargo test --test git_network_integration -- --test-threads=1
cargo test --test interactive_process_attach_resume -- --test-threads=1
cargo test --test interactive_process_sessions -- --test-threads=1
cargo test --test git_mutations_integration -- --test-threads=1
cargo test --test git_recovery_integration -- --test-threads=1
cargo test --test command_routing_execution_ownership -- --test-threads=1
cargo test --test tool_execution -- --test-threads=1
cargo test --test tool_broker_integration -- --test-threads=1
cargo test -p codegg --lib tool::broker -- --test-threads=1
cargo test -p codegg --lib scheduler -- --test-threads=1
cargo test -p codegg --lib live_execution_audit -- --test-threads=1
cargo test -p codegg --lib m002_audit_label -- --test-threads=1
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

### Results

- `identity_m002_live_audit_hooks`: pass, 17 passed (shell
  success; failure/timeout/denial matrix; retry no-dup + distinct
  invocation; terminal native path; scheduler test success;
  scheduler cancel + interrupt distinguishability; test-tool
  no-scheduler negative; interactive create + lifecycle silence +
  input-secret negative; git stage/branch; git network push/fetch +
  credential-URL failure negative; git recovery; bash-routed typed
  convergence; bash managed-plumbing single shell event; git child-
  policy denial; git read-only negative; git replay no-dup;
  ownership matrix).
- `identity_m005_audit_instrumentation`: pass, 13 passed (no
  builder/fixture regression).
- `git_execution_origin_matrix`: pass, 28 passed (no routing or
  origin regression).
- `git_credential_cross_path`: pass, 14 passed (no redaction
  regression).
- `git_network_integration`: pass, 29 passed.
- `interactive_process_attach_resume`: pass, 19 passed (create
  signature change is behavior-preserving without a hook).
- `interactive_process_sessions`: pass, 11 passed.
- `git_mutations_integration`: pass, 12 passed.
- `git_recovery_integration`: pass, 19 passed.
- `command_routing_execution_ownership`: pass, 21 passed.
- `tool_execution`: pass, 55 passed.
- `tool_broker_integration`: pass, 25 passed.
- `codegg --lib tool::broker`: pass, 7 passed (4 pre-existing + M001
  preservation/no-synthesis + M002 emitter-threading test).
- `codegg --lib scheduler`: pass, 78 passed.
- `codegg --lib live_execution_audit`: pass, 5 passed (ownership
  matrix, scope keys, argv boundary digest, executor shapes,
  planned shapes).
- `codegg --lib m002_audit_label`: pass, 4 passed (bounded labels,
  read-only exclusion, URL-free digests, managed-argv preimage).
- `check_git_forbidden_patterns.py`: pass (0 findings).
- `check_audit_coverage.py --verbose`: pass, 7/7 (6/6 pre-existing
  checks plus the new M002 live-hook check).
- `check_execution_ownership.py`: pass (no new spawn surface; audit
  emission is not execution).
- `check_scheduler_bypass.py`: pass.
- `scripts/verify.sh quick`: pass (`==> Quick verification passed.`;
  covers fmt, agent schema, core-boundary, sandbox,
  execution-ownership, tui-authority, http-route-disposition,
  audit-coverage, scheduler-bypass, workspace check).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`:
  pass, 0 warnings.
- `git diff --check`: pass.

Substitutions recorded (plan §6): the plan names
`interactive_process_m001`, which does not exist as a test target;
coverage is provided by `interactive_process_attach_resume` (19
passed, exercises the changed `create` signature without a hook)
plus `interactive_process_sessions` (11 passed) plus the new
`interactive_create_emits_without_input_retention` live-hook test.
Timeout outcomes stand in for scheduler-driven cancellation on the
bash/terminal surfaces (no cancellation token exists there); live
cancelled/uncertain outcomes are exercised through the scheduler
test executor instead.

All evidence above is local execution and is labeled accordingly. No
hosted `CI / verify` run is attached; routine CI remains the operator
action per the closed development-verification roadmap.

## 5. Invariant review

- Actor/provenance derives from trusted transport/daemon state,
  never caller/model strings: tools emit only via
  `live_audit_hook` (context + emitter threaded per invocation);
  the broker projects origin strings only from the trusted context;
  interactive attribution comes from the registry-bound principal;
  git labels/digests derive from typed operation fields, never URLs,
  messages, paths, or output.
- Coordinator remains sequence/store authority: every hook emits
  through `ExecutionAuditEmitter` (one bounded timeout, existing
  counters/warn); no second database, bus, or queue; deterministic
  ids reuse stored rows via `ON CONFLICT DO NOTHING`.
- No command/argv/input/URL/file/tool-output content in audit
  state: digests are SHA-256 hex; labels are bounded literals;
  metadata-negative tests assert the absence on every matrix row;
  the store secret guard still rejects secret-bearing
  keys/values/bodies.
- One transition emits at most one event: each owner emits once per
  executed transition; the broker emits nothing; the Git route
  silences the shell layer; managed plumbing silences the executor;
  replays dedupe by deterministic id (tested).
- Bounded best-effort failure: emitter timeouts/failures increment
  counters with warn logs and never fail the owning operation
  (inherited M001 policy, unchanged).
- Execution ownership not bypassed: bash still routes through the
  scheduler-owned paths; git mutations still execute through
  `GitMutationExecutor`; interactive lifecycle still runs on the
  scheduler admission controller; execution-ownership and
  scheduler-bypass guards pass; `docs/execution-ownership.toml`
  needed no change (no new spawn surface).
- Personal-local and team execution share one composition: same
  context/emitter/hook types; unthreaded paths stay silent rather
  than inventing identity.

## 6. Failure and recovery review

- Pool-less daemon: emitter without a pool drops with
  `dropped_no_pool` (M001-tested); hooks stay silent, execution
  unaffected.
- Store failure / timeout: `failed` counter increments with warn;
  the owning command/git/interactive operation completes (or fails)
  on its own terms; audit never changes control flow.
- Retry/replay: deterministic ids per (decision, action,
  correlation, scope) return stored rows; tested for bash
  invocation-key replays and git replay scopes.
- Cancellation races: timeout arms emit before propagating;
  pre-spawn cancellations/denials emit nothing; no background task
  or queue to drain on shutdown.
- Malformed/unauthorized input: unknown tools map to no family
  (`None`); unknown git subcommands map to `unknown` or no event
  (read-only); `principal_ref`-only callers still receive no trusted
  context (M001-tested, unchanged).

## 7. Migration and compatibility review

- No storage migration: no schema change; catalog layout unchanged.
- No protocol change: no new `CoreRequest`/`CoreResponse` variants
  (create responses unchanged), no `PROTOCOL_VERSION` bump; the
  interactive hook and audit contexts have no `serde` impls.
- Backward compatible: all new context fields are `Option` with
  `None` defaults; `protocol.create` gains a trailing
  `Option<InteractiveAuditHook>` (harness callers pass `None`);
  tool `execute()` behavior without a context is byte-identical
  (existing suites green).
- Rollback: dropping the M002 commit restores silent hooks; no
  durable state depends on the new events (rows already written
  remain ordinary queryable audit rows).

## 8. Security review

- No new authority: emitters perform no authorization decisions;
  hooks fire only after the owning allow/admission decision.
- No principal fabrication: contexts come from daemon-owned state
  or explicit `legacy_local`; the interactive hook uses the
  handshake-bound principal with explicitly daemon-asserted (not
  gate-copied) provenance, documented as such.
- No wire injection: no `serde` on contexts/hooks; DTOs cannot
  supply them.
- Secrets: digests only (SHA-256 hex), labels bounded; URL scrub
  defense-in-depth before git digest preimages; existing
  secret-negative suites still green plus new per-row negatives.
- Denial-of-service bounds: one event per executed transition, no
  unbounded queues, no new network/spawn surface; counters expose
  pressure.
- `codegg-core` boundary guard passes (no core production changes
  in M002 at all); scheduler/Git/broker/tools remain in the root
  crate consuming core types only.

## 9. Documentation and operations

Updated:

- `architecture/audit.md` — "Live execution hooks (M002)":
  ownership matrix, shell/test/process emission, interactive
  create hook, git hook semantics, guard/test pointers; gap list
  narrowed to `job_complete` (M003).
- `architecture/git.md` — executor row now documents the live hook.
- `scripts/check_audit_coverage.py` — executable M002 hook pins
  (7/7).
- `plans/implementation/identity-audit-live-execution-post-closure-corrective/002-command-interactive-git-live-audit-hooks.md`
  — marked closed, linking this record.
- `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md`
  — M002 `ready` → `closed`.
- `plans/registry.md` — subsystem row, dependency-ready table,
  blocked work, execution-order gate, closure-evidence table (see
  §12).

Operator note: no migration, no config change. `appended` will grow
as threaded contexts reach the hooks (turn/job boundaries still
thread `None` today, so production volume is unchanged until
M003/M004 supply attribution). `failed` growth still means audit
I/O saturation — shed load rather than retrying with fresh ids.

## 10. Unresolved findings

There are no unresolved critical, high, medium, or low M002 findings.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | — | — | — |

Explicit non-claims with named consumers (not defects):

- Live scheduler `job_complete` remains M003 scope against this seam.
- Guard/trajectory qualification remains M004 scope after M003.
- Production turn/job attribution threading (replacing today's
  `None`) rides with M003/M004, not this hook milestone.
- Distributed `node_enrollment` / `remote_execute` remain future scope.

No stop condition fired: sequence/store authority stays with the
coordinator; no second audit database or event bus; no trusted
principal/provenance in public untrusted DTOs; no authorization-model
change.

## 11. Roadmap disposition

Milestone closed and dependencies partially advance:

- M002 `ready` → `closed`.
- M003 stays `ready`: its sole hard dependency (M001 seam) was
  already satisfied; M002 does not change it.
- M004 stays `blocked`, narrowed from "M002+M003 closure" to "M003
  closure": this closure satisfies the M002 leg; the scheduler
  terminal hook (M003) is the remaining hard dependency.

No corrective pass is required. No downstream plan is newly
unblocked (M003 was already `ready`; M004 still waits on M003) —
this is the partial-unblock case: the blocker description is
updated, not removed.

## 12. Registry updates

Included in the closure commit alongside this record:

- M002 source plan marked closed, linking this record.
- Roadmap milestone table: M002 `ready` → `closed` with closure link;
  M003 stays `ready`; M004 stays `blocked` on M003.
- Registry active-subsystem row: Identity current milestone `M001
  closed; M002+M003 ready; M004 blocked` → `M001+M002 closed; M003
  ready; M004 blocked`; blocker column narrowed to the remaining
  scheduler/qualification dependencies.
- Registry dependency-ready table: M002 row `ready` → `closed` with
  closure link.
- Registry execution-order gate: post-closure cleanup gate advanced
  from "M001 closed; M002+M003 ready; M004 blocked on M002+M003" to
  "M001+M002 closed; M003 ready; M004 blocked on M003".
- Registry blocked-work table: M004 blocker narrowed from
  "M002+M003 closure (M001 satisfied and closed)" to "M003 closure
  (M001+M002 satisfied and closed)".
- Registry closure-evidence table: Identity M002 row added pointing at
  this record with implementation `e0cc7f05`.