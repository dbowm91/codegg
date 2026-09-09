# Identity, Authorization, and Audit Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/identity-authorization-audit/005-audit-instrumentation-attribution-closure.md`

Source subsystem roadmap:

- `plans/subsystems/identity-authorization-audit-roadmap.md#M005--audit-instrumentation-and-attribution-closure`

Repository baseline reviewed: `bd8fae25` (`feat(identity): M005 audit instrumentation and attribution closure`)

Implementation commits:

- `bd8fae25` — M005 audit instrumentation and attribution closure:
  canonical event-coverage matrix and typed builders
  (`crates/codegg-core/src/audit_instrumentation.rs`), daemon
  post-authorization seam with best-effort bounded emission
  (`src/core/daemon.rs`), end-to-end attribution fixtures
  (`tests/identity_m005_audit_instrumentation.rs`), coverage guard
  (`scripts/check_audit_coverage.py`), architecture docs
  (`architecture/audit.md`), unit + boundary integration tests.

## 1. Executive finding

M005 is closed. The coordinator instruments the required Phase-11
authentication/authorization/membership/session/prompt/provider/model/
agent/permission/tool/file/worktree/job/config/asset/audit surfaces
through one executable event-coverage matrix using typed builders at
canonical owners. A project owner can query a representative operation
and trace it to the authenticated principal, the authorization
decision, the project/session/turn, agent descendants, jobs/tools, and
worktree outcome as applicable; required privileged actions have named
audit owners; secrets never reach storage or export; high-volume paths
remain bounded. No unresolved high, medium, or low M005 finding
remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Executable event-coverage matrix mapping action to owner/actor/scope/decision/metadata (work package A) | `crates/codegg-core/src/audit_instrumentation.rs`: `REQUIRED_AUDIT_COVERAGE` (25 rows, one per `AuditAction::ALL`), `coverage_for_action`, `INSTRUMENTED_OPERATIONS` (63 ops), `UNINSTRUMENTED_OPERATIONS` (76 reads/infra) | pass | Owners name canonical emitters (`daemon:session`, `daemon:scheduler`, …); actor is always transport-bound; metadata lists allowed structural keys only. |
| Identity/session/provider/config/asset control-plane instrumentation (work package B) | Daemon `emit_audit_for_authorized` post-gate seam + post-creation emits for `SessionCreate`/`SessionImportData`/`SessionCreateFromTemplate` (durable session id), `SessionSelectionUpdate` (connection/model), `JobSubmit` (durable job/session/turn), `AuditQuery`/`AuditExport` (returned count, post-read) | pass | Creation/audit-read ops skip generic pre-emit and emit post-handler with durable ids/counts so envelopes never contain their own event. |
| Agent/tool/process/file/Git/worktree/job execution causation (work package C) | Typed builders `prompt_submit`/`agent_delegate`/`tool_invoke`/`command_execute`/`file_mutate`/`git_operation`/`worktree_lifecycle`/`job_submit`/`job_cancel`/`job_complete` with `AuditChainContext` correlation + `causation_parent`; daemon maps `TurnSubmit`->prompt (digest), `AgentSelect`/`Goal*`->delegate, `RunRerun`->tool, checkpoints/`LspPreviewApply`->file, worktree cleanup/archive->worktree, `JobCancel`/`Schedule*`->job terminal | pass | `command_execute`/`git_operation` live executor hooks deferred by design (builders + fixtures landed); file/worktree/job/tool control-plane intents emit live. |
| End-to-end attribution fixtures and high-volume/negative secret tests (work package D) | `tests/identity_m005_audit_instrumentation.rs` (13 tests): auth/denial/membership chain; prompt->delegate->tool/job->Git/worktree chain with shared correlation + parent links; provider/model; permission allow/deny; cancel/failure terminal; asset/config keys-not-values; secret-negative (label/value/body); deterministic retry; bounded writer; daemon reader auth; daemon denial terminal; daemon self-describing read; matrix coverage | pass | Fixtures assert ordered actions, shared correlation, parent linkage, digest-only bodies, and authorized reads. |
| Docs/coverage guard ensuring new privileged operations cannot land unclassified (work package E) | `scripts/check_audit_coverage.py` (5/5): matrix covers every action; every daemon operation classified; live-mapped actions have mappings (`authorization_decision` via denial seam); append-only + trusted attribution pinned; operator matrix doc exists | pass | `UNINSTRUMENTED_OPERATIONS` pinned so a new mutating operation fails the guard unless mapped. |
| Permission/authorization decision references captured | Every builder carries `AuditDecisionProvenance` (decision id, correlation, policy, project); denials emit `authorization_decision` with operation/capability/reason | pass | No authority setters exist; request DTOs supply locators only. |
| Counters/diagnostics for bounded audit failure/backpressure | `AuditWriterMetrics` (M004) plus process-wide `emit_counters_snapshot` (`appended`/`failed`/`dropped_no_pool`); daemon appends bounded by 500 ms timeout with warn logs, never failing the operation | pass | Saturation surfaces `audit_backpressure`/`audit_write_timeout`, never silent success. |

## 3. Production implementation evidence

- `crates/codegg-core/src/audit_instrumentation.rs` (new, ~1340 lines
  with tests): `AuditCoverageEntry` + `REQUIRED_AUDIT_COVERAGE` (25
  rows with owner/actor/scope/decision/metadata/causation/visibility/
  live flag), `coverage_for_action`, `INSTRUMENTED_OPERATIONS` (63) +
  `UNINSTRUMENTED_OPERATIONS` (76) + `operation_to_audit_action`,
  `AuditChainContext` (+ `child_of`), `deterministic_event_id`
  (SHA-256 hex idempotency), 20 typed builders accepting only
  structural locators/digests/labels/outcomes, `structural_digest`,
  process-wide emit counters, 8 focused tests. Boundary-clean: no
  `crate::authorization` import (daemon owns the
  operation-descriptor + provenance-bridge step).
- `crates/codegg-core/src/lib.rs`: `pub mod audit_instrumentation;`.
- `src/core/daemon.rs` (+~640 lines): `append_audit_event`
  (best-effort, 500 ms timeout, counters + warn),
  `audit_chain_for_request` (session/turn/job/worktree/provider/run
  locators from DTOs + gate-resolved project),
  `emit_audit_for_authorized` (post-gate, pre-effect; skips
  creation/audit-read ops handled post-handler; per-action builders
  with prompt digest, connection/model, checkpoint digest, worktree
  op, job terminal outcomes), `emit_audit_for_denial` (terminal
  denied event with direct-project preservation where parsable),
  post-creation emits for session creation (3 arms), provider
  selection, job submission, audit query/export (returned counts).
- `tests/identity_m005_audit_instrumentation.rs` (new, 13 tests, no
  required-features gate): see §2 work package D and §4.
- `scripts/check_audit_coverage.py` (new, 5/5 green): matrix
  completeness, operation classification, live-mapping consistency,
  append-only + trusted attribution, operator doc presence.
- Docs: `architecture/audit.md` — M005 coverage contract
  (matrix ownership, live daemon seam, correlation/causation,
  digests-only bodies, explicit gaps, backpressure, operator reads,
  verification).

Distinguished as absent (explicit gaps, not defects):

- Live tool-broker `command_execute` and git-executor
  `git_operation` hooks: builders + store fixtures landed and
  chained; live executor emission deferred (low severity, §10).
- Live async scheduler `job_complete` hook: `job_retry` maps live as
  the representative terminal; full async completion chains proven
  via builders/fixtures (low severity, §10).
- `node_enrollment`, `remote_execute`, `chat_triggered_action`:
  builders landed, no live single-host emission by plan scope
  (remote/chat out of scope; collaboration M003 owns chat actions).

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib audit
cargo test --test identity_m005_audit_instrumentation
cargo test --test identity_m004_audit_foundation
cargo test --workspace audit --no-fail-fast
cargo test --workspace agent --no-fail-fast
cargo test --workspace worktree --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_audit_invariants.py --verbose
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_project_catalog_invariants.py --verbose
bash scripts/check-core-boundary.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_tool_broker_boundary.py
python3 scripts/audit_tokio_tests.py
bash scripts/verify.sh quick
```

### Results

- `cargo test -p codegg-core --lib audit`: pass, 22 passed
  (14 M004 store/writer/taxonomy + 8 M005 matrix/mapping/
  deterministic/chain/secret/digest tests).
- `cargo test --test identity_m005_audit_instrumentation`: pass, 13
  passed (auth/denial/membership chain; prompt->delegate->tool/job->
  Git/worktree causation; provider/model; permission allow/deny;
  cancel/failure terminal; asset/config keys; secret-negative;
  deterministic retry; bounded writer; daemon reader auth; daemon
  denial terminal; daemon self-describing read; matrix coverage).
- `cargo test --test identity_m004_audit_foundation`: pass, 13
  passed (no M004 regression).
- `cargo test --workspace audit --no-fail-fast`: pass, 0 failures
  across the sweep.
- `cargo test --workspace agent --no-fail-fast`: pass, 0 failures.
- `cargo test --workspace worktree --no-fail-fast`: pass, 0 failures.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features --
  -D warnings`: pass, 0 warnings (4 test-only
  `unnecessary_get_then_check` lints found during development and
  fixed before the final run).
- `python3 scripts/check_audit_invariants.py --verbose`: pass, 6/6.
- `python3 scripts/check_audit_coverage.py --verbose`: pass, 5/5.
- `python3 scripts/check_authorization_matrix.py --verbose`: pass,
  5/5.
- `python3 scripts/check_project_catalog_invariants.py --verbose`:
  pass, 7/7 (including `STORAGE_LAYOUT_VERSION is 54`; no M005
  migration — store unchanged).
- `bash scripts/check-core-boundary.sh`: pass (instrumentation
  module carries no `crate::authorization` import; daemon owns the
  gate bridge).
- `python3 scripts/check_daemon_cwd_usage.py`: pass.
- `python3 scripts/check_scheduler_bypass.py`: pass.
- `python3 scripts/check_tool_broker_boundary.py`: pass.
- `python3 scripts/audit_tokio_tests.py`: exit 0; advisory
  candidates only in pre-existing files, none in M005 files.
- `bash scripts/verify.sh quick`: pass (`==> Quick verification
  passed.`).

All evidence above is local execution and is labeled accordingly. No
hosted `CI / verify` run is attached; routine CI remains the operator
action per the closed development-verification roadmap.

## 5. Invariant review

- Instrumentation is emitted by canonical owners: daemon dispatch
  arms (control-plane) plus typed builders consumed at those arms;
  no duplicate wrapper emits the same transition twice (creation and
  audit-read ops emit once post-handler; all others emit once
  pre-effect).
- Actor/correlation derives from trusted context: every builder takes
  the transport-bound principal plus the gate-copied provenance;
  chain locators come from DTO locators + gate-resolved project, never
  from caller-supplied authority.
- Audit failure policy preserved: emits are best-effort with a
  bounded timeout; saturation surfaces typed backpressure/timeout
  with counters and warn logs, never silent success or unbounded
  queues.
- Secrets/body retention rules uniform: builders accept digests,
  ids, bounded labels, and outcome enums only; prompt/file/output
  bodies are never parameters; the store still rejects
  secret-bearing keys/values/bodies before any write
  (corpus-pinned in both M004 and M005 suites).
- High-volume paths bounded: reads/listings stay uninstrumented by
  explicit allowlist; each emit is one bounded append; writer
  semaphore/timeout bounds concurrent pressure.
- Chat remains separate: no live chat emission; the
  `chat_triggered_action` builder exists for the collaboration
  milestone without a daemon mapping.

## 6. Failure and recovery review

- Cancellation/completion produce terminal structural events:
  `job_cancel` (request + causation to submit) and `job_complete`
  (retry live; failure/interrupted via builders) carry terminal
  outcomes; duplicate/replayed transitions reuse
  `deterministic_event_id` and return the stored event without a
  second sequence (tested).
- Audit pressure does not reorder canonical state: emits run after
  the gate decision and never gate execution on audit I/O; timeouts
  warn without failing the operation.
- Denials emit terminal `authorization_decision` events with the
  supplied operation/capability and reason; directly-scoped denials
  stay queryable by the project owner through `audit.read`;
  unscoped denials stay `None`-projected and never leak existence
  through pages.
- Malformed input degrades safely: unknown operations emit nothing
  rather than fabricating attribution; unknown action filters
  degrade to empty pages per M004.

## 7. Migration and compatibility review

- No storage migration: M005 reuses migration v54
  (`STORAGE_LAYOUT_VERSION` stays 54; catalog guard still 7/7).
- No protocol change: no new `CoreRequest`/`CoreResponse` variants,
  no `PROTOCOL_VERSION` bump. Audit DTOs unchanged; new events flow
  through existing `AuditEventDto` pages/exports.
- Operation matrix unchanged in `authorization.rs`; the new
  `operation_to_audit_action` table is additive and pinned by the
  coverage guard. Existing M003/M004 suites pass unmodified.

## 8. Security review

- Fail-closed reads preserved: team principals still need a current
  `audit.read` grant on the queried project; `Viewer`/`Contributor`
  denials tested live at the daemon gate for both query and export
  paths (M005 suite) in addition to M004.
- Denial events contain no secret material and no existence signal
  beyond operation/capability names the caller supplied plus the
  denial reason from the typed error.
- Redaction runs before the write on every path: secret-bearing
  labels, values, and bodies rejected with `audit_secret_detected`
  (M005 secret-negative test covers label/value/body; count stays 0).
- Privilege boundaries unchanged: only `Active` principals
  authorize; `LocalOwner` broad policy remains an explicit
  composition visible in every event's `policy` field.
- Denial-of-service bounds: per-write 500 ms daemon timeout, writer
  semaphore/timeout, metadata/body caps, query/export clamps, no new
  network or spawn surface.
- `codegg-core` boundary guard passes; the instrumentation module
  imports only `audit`, `identity`, `transport_auth`, and base
  crates.

## 9. Documentation and operations

Updated:

- `architecture/audit.md` — M005 coverage contract: matrix
  ownership, live daemon seam (pre-effect + post-creation +
  denial), correlation/causation, digests-only bodies, explicit
  gaps, backpressure/counters, operator reads, verification.
- `scripts/check_audit_coverage.py` — executable M005 coverage
  guard (5/5).
- `plans/implementation/identity-authorization-audit/005-audit-instrumentation-attribution-closure.md`
  — marked closed, linking this record.
- `plans/subsystems/identity-authorization-audit-roadmap.md` — M005
  `ready` → `closed` with closure link; roadmap `active` → `closed`.
- `plans/registry.md` — Identity row advanced to closed; M005
  removed from dependency-ready; execution-order chain advanced to
  M005 closed; collaboration blocker narrowed to presence M003;
  closure evidence extended with the M005 row.

Operator note: after upgrading, no migration runs (store stays v54).
New structural events appear for authorized control-plane operations
and denials automatically; no operator action is required. Team
operators should grant `Maintainer` or `Owner` to members who need
audit reads (`Viewer`/`Contributor` cannot query or export, and
their denials are themselves recorded for the project owner).
Content bodies were never required for attribution; verify exports
with `export_digest` over the received order. Monitor both
`AuditWriterMetrics` and the new process-wide emit counters; rising
`failed` means audit I/O is saturated — shed load rather than
retrying with fresh event ids (reuse ids for idempotent retry).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Live `command_execute` hook lives only in builders/fixtures; no daemon tool-broker emission yet. | Command-level chains queryable via fixtures; live control-plane covers the `run_rerun` tool intent but not per-command digests from the broker. | Future tool-broker pass should call `command_execute_event` with the argv digest at the canonical dispatch owner. Guard currently pins `command_execute` as explicitly non-live. |
| Low | Live `git_operation` hook lives only in builders/fixtures; no daemon git-executor emission yet. | Git/worktree chains queryable via fixtures and worktree-lifecycle live events; per-commit/ref live digests not yet emitted. | Future git pass should call `git_operation_event` with op + ref digest at the canonical mutation owner. Guard pins `git_operation` as non-live. |
| Low | Live async scheduler `job_complete` hook lives only in builders/fixtures; `job_retry` is the live representative terminal. | Submit/cancel terminals emit live with causation; completion success/failure shapes proven in fixtures. | Future scheduler pass should call `job_complete_event` at the terminal attempt transition with the stored attribution. |

There are no unresolved critical, high, or medium M005 findings.
The following are explicit non-claims with named consumers, not
defects:

- Remote/node sequencing (`node_enrollment`, `remote_execute`) and
  structured chat actions (`chat_triggered_action`) remain out of
  scope per the plan; builders landed, no live emission, owned by
  future distributed-execution and collaboration milestones.
- One boundary-driven design adjustment is recorded, not concealed:
  the instrumentation module performs no authorization-module calls
  because `codegg-core` forbids `crate::auth*` imports
  (`scripts/check-core-boundary.sh`). The daemon seam owns the
  `operation_descriptor` → `operation_to_audit_action` step plus the
  `audit_provenance` bridge, preserving typed decision linkage
  without breaking the boundary.

## 11. Roadmap disposition

Milestone closed and the subsystem roadmap closes with it: M001
through M005 now all have accepted closure records, satisfying the
roadmap completion definition (team network requests individually
attributable and project-authorized, LocalOwner frictionless,
required structural activity append-only auditable without secret
leakage).

The registry audit found no newly unblocked plan whose sole hard
blocker was M005 closure:

- Project collaboration M001 remains blocked: its blocker is
  identity/audit M005 **plus** presence-observation M003. M005 is now
  satisfied; presence M003 (blocked on presence M001/M002) is not.
  The registry blocker is narrowed to the remaining presence
  dependency rather than cleared.
- Presence and all other tracks are unaffected by this closure.

No corrective pass is required and no new dependency-ready plan was
created beyond the unblock audit.

## 12. Registry updates

Included in the closure commit alongside this record:

- M005 source plan marked closed, linking this record.
- Roadmap milestone table: M005 `ready` → `closed` with closure
  link; roadmap status `active` → `closed`.
- Registry active-subsystem row: Identity current milestone M004
  closed / M005 ready → M005 closed; status `active` → `closed`;
  blocker column cleared to the subsystem-closure note.
- Registry dependency-ready table: M005 instrumentation row removed
  (closed, no longer ready); remaining five ready plans retained
  unchanged.
- Registry execution-order item 2: chain advanced to M005 closed
  (subsystem complete).
- Registry blocked-work table: collaboration M001 blocker narrowed
  from identity M005 + presence M003 to presence M003 alone
  (identity M005 closed); all other rows retained unchanged.
- Registry closure-evidence table: Identity M005 row added pointing
  at this record.
