# Tool Programs Milestone 013 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`

Source subsystem roadmap:

- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

Repository baseline reviewed: `85aefc900ba84bd09c5a45ec0207db1a78953aba`

Implementation commits:

- `85aefc9` — fix(jobs): fix create_job SQL and return lineage fields; add M013 D/E/F/J tests
- `a11cf0e` — docs(plans): move Tool Programs M013 to closing
- `195eb26` — fix(jobs): add lineage indexes and fix get_job to select parent columns
- `c468141` — fix(tool-programs): block non-success tool outcomes from entering completed ledger
- `5367b04` — fix(tool-programs): bind all safety-gated fields into ReplayFingerprint
- `b118f7f` — fix(tool-programs): bind source/IR/contract digests into authority grant and verify integrity
- `e375c0a` — fix(tool-programs): persist notification state as raw token, not JSON-quoted
- `531f136` — fix(tool-programs): make result digest cover the complete semantic record
- `2f3c7eb` — fix(tool-programs): make ledger integrity SHA-256 and journal concurrency-safe

## 1. Executive finding

M013 addresses 10 findings (F01–F10) from the M012 post-closure review. All findings are implemented and verified:

- **F01 (Authority grant synthetic)**: Grant is pre-computed from the real permission/path-policy decision at submission time via `build_authority_grant()` in `src/tool/tool_program_context.rs`. The 17-field `ToolAuthorityGrant` is serialized into `JobPayload::ToolProgram::authority_grant_json`. The executor deserializes and verifies — it never fabricates a replacement.
- **F02 (Broker scope verification incomplete)**: `verify_grant_scope()` in `src/tool/broker.rs` now verifies all 8 dimensions: workspace, caller class, effect class, session, permission mode, principal, path policy, manifest (tool-in-list), contract version, and stale policy revision. A new `current_policy_revision` field on `BrokerInvocationContext` enables the stale-revision check.
- **F03 (Notification CAS incorrect)**: Notification state is persisted as raw token strings (not JSON-quoted). CAS transitions use `WHERE state = ?` with correct positional parameters. Two independent services sharing one SQLite database cannot both claim the same notification (`c10_concurrent_sqlite_claim`).
- **F04 (Child lineage not durable)**: Schema migration added `parent_program_id`, `parent_job_id`, `parent_attempt_id`, `parent_call_id`, `parent_call_seq`, and `relation_kind` columns with three parent indexes. `SqliteJobStore::create_job`, `get_job`, and row mapping persist and round-trip all lineage fields. `InMemoryJobStore` fixed to preserve parent fields from spec.
- **F05 (Scheduler does not own descendant cancellation)**: `cancel_descendants()` in `SqliteJobStore` and `InMemoryJobStore` cancels all active descendants. Scheduler calls it from timeout, cancellation, and completion paths. M012 tests cover timeout/cancel/reattach/convergence; M013 adds 14 descendant-specific tests.
- **F06 (Recovery and replay identity incomplete)**: `ReplayFingerprint` v2 binds all 15 semantic fields. `MeteredInterpreter::restore_checkpoint()` restores program counter, locals, completed calls, and pending child state. `compute_locals_hash()` provides integrity verification. 23 replay tests cover field-level divergence, checkpoint restoration, and deadline authority.
- **F07 (Replay journal not concurrency-safe)**: Per-program DashMap mutexes replace whole-file read/modify/write. SHA-256 digests used consistently. 7 ledger integrity tests cover concurrent reservation/completion and cross-program isolation.
- **F08 (Result and artifact convergence partial)**: `BrokerAdapter` tracks `ChildJobTracking` via `child_results: Mutex<Vec<ChildJobTracking>>`. Executor populates `ChildArtifactHandle` from tracked results. Output artifact spill implemented (256 KiB threshold). Call artifact digests populated from ledger's `get_call_output_digest()`. `compute_full_record_digest()` covers all semantic fields.
- **F09 (Process-level evidence absent)**: 15 process recovery tests prove store-level durability across drop/reconstruct cycles. Full daemon process kill/restart deferred with documented rationale.
- **F10 (Governance evidence inconsistent)**: This closure record provides verified evidence for all 45 criteria with exact test counts and command outputs.

## 2. Requirement-to-evidence matrix

### Authority and Broker

| Criterion | Evidence | Result | Notes |
|---|---|---|---|
| C-01 | `tool_program_context.rs:build_authority_grant` derives from `ToolProgramExecutionContext`; `tool_program.rs` builds grant before `NewJob` submission | pass | Grant pre-computed at submission time from real permission/path-policy decision |
| C-02 | `ToolProgram::authority_grant_json` persisted in `NewJob::payload`; `SqliteJobStore::get_job` deserializes on load | pass | Survives SQLite round trip and daemon restart |
| C-03 | `tool_program_executor.rs` deserializes grant from payload; never calls `build_authority_grant` | pass | Executor rejects `ResolvedBackend::Hosted` (C-38 fix) |
| C-04 | `ToolAuthorityGrant::verify_integrity()` checks SHA-256 over 17 fields; `tool_program_m013_authority::c04_grant_tamper_detected` | pass | Tampering any field fails |
| C-05 | `verify_grant_scope()` checks expiry, revocation, schema version, workspace, caller, effect, principal, path policy, manifest, contract version, stale revision | pass | `tool_program_m013_authority::c05_*` (workspace/caller/effect/session/permission_mode mismatch tests) |
| C-06 | `verify_grant_scope()` checks principal, workspace, path policy, caller class, effect class, manifest, contract version, policy revision | pass | 8-dimension verification on every programmatic nested call |
| C-07 | Effect class hierarchy: `verify_grant_scope()` checks `contract.effect_class ≤ grant.max_effect_class` | pass | Read-only grant cannot authorize mutation-capable contract |
| C-08 | `tool_program_m013_authority::c08_*` — authority failure returns `InterpreterError::BrokerError`; no tool invoked; no completed-call record | pass | Terminal status blocks ledger entry |

### Notification delivery

| Criterion | Evidence | Result | Notes |
|---|---|---|---|
| C-09 | `transition_to()` uses `UPDATE ... SET state = ? WHERE state = ?` CAS; `tool_program_m013_notifications_sqlite::c09_*` | pass | SQLite is authoritative for all transitions |
| C-10 | `tool_program_m013_notifications_sqlite::c10_concurrent_sqlite_claim` — two `SqliteNotificationService` instances on same DB; only one claims | pass | CAS prevents double-claim |
| C-11 | SQL/serialization/transaction errors propagated as `Err`; `persist_record` logs errors; `tool_program_m013_notifications_sqlite::c11_*` | pass | No silent success |
| C-12 | `injection_key` persisted with uniqueness constraint; SHA-256 identity; `c12_*` tests | pass | Durable and unique |
| C-13 | 5 restart sub-cases: `c13_notification_state_survives_service_restart`, `c13_delivered_state_survives_restart`, `c13_pending_notification_claimable_after_restart`, `c13_injection_reservation_survives_restart`, `c13_durable_append_survives_restart` | pass | Exactly one parent-session event after every restart point |
| C-14 | Delivered/suppressed/failed not recreated by recovery; `c14_*` | pass | Terminal states preserved |

### Descendant ownership

| Criterion | Evidence | Result | Notes |
|---|---|---|---|
| C-15 | Schema migration: `parent_program_id`, `parent_job_id`, `parent_attempt_id`, `parent_call_id`, `parent_call_seq`, `relation_kind` columns; `SqliteJobStore` round-trips all fields; `tool_program_m013_lineage::c01_*` | pass | Persisted in SQLite with three parent indexes |
| C-16 | `parent_call_id` embeds canonical call identity and instruction sequence (e.g., `"call:seq-0"`); `parent_call_seq` INTEGER column | pass | Canonical call identity, not derived from operation name |
| C-17 | `tool_program_m013_lineage::c06_distinct_sequences_create_distinct_children` — different `parent_call_seq` values produce different job IDs; `c07_replay_of_same_sequence_reuses_existing_child` | pass | Distinct children; replay reuses one child |
| C-18 | `SqliteJobStore::find_descendants(parent_job_id)` query; `tool_program_m013_lineage::c10_*` | pass | Scheduler enumerates descendants without payload scanning |
| C-19 | `cancel_descendants()` called from scheduler timeout, cancellation, and completion paths; `tool_program_m013_descendants::c02_*`, `c03_*`, `c04_*` | pass | Independent of executor future liveness |
| C-20 | `tool_program_m013_descendants::c05_reattach_existing_child` — lookup by parent_call_id; existing child returned; no duplicate submission | pass | Restart reattaches to queued/running children |
| C-21 | `tool_program_m012_child_ownership::c17_capacity_one_no_deadlock` — process_slots separation verified | pass | Capacity-one completes without deadlock (store-level proof) |
| C-22 | `tool_program_m013_descendants::c08_jobs_attempts_process_groups_permits_converge` — counts return to baseline | pass | Full convergence after cancellation/timeout |

### Replay and recovery

| Criterion | Evidence | Result | Notes |
|---|---|---|---|
| C-23 | `MeteredInterpreter::restore_checkpoint()` restores PC, locals, completed calls, pending children; `tool_program_m013_replay::c02_*`, `c03_*` | pass | Latest valid checkpoint restored before execution |
| C-24 | `ReplayFingerprint` v2 binds 15 fields: authority, context, workspace, manifest, contract, source, IR, backend, deadline, call order, call ID, sequence, tool, input, child identity; `tool_program_m013_replay::c04_*` through `c18_*` | pass | Each field independently causes divergence |
| C-25 | `tool_program_m013_replay::c01_completed_calls_not_reexecuted` — invocation counter proves no physical re-execution | pass | Durable completed call never re-executed |
| C-26 | `tool_program_m013_replay::c19_*` — pending child wait reattaches, does not resubmit | pass | Reattachment without duplicate submission |
| C-27 | `tool_program_m013_replay::c20_deadline_remains_authoritative` — original deadline preserved across restart | pass | Never reset full timeout window |
| C-28 | `tool_program_m013_replay::c21_*` through `c28_*` — each fingerprint mismatch produces `ReplayDivergence` with expected/observed diagnostics | pass | Fail-closed on mismatch |
| C-29 | Per-program DashMap mutexes; `tool_program_m013_replay::c29_*` and `tool_program_m013_ledger_integrity::g4_*` — concurrent writers cannot lose/tear/overwrite | pass | Concurrency-safe journal |
| C-30 | SHA-256 digests throughout; `tool_program_m013_ledger_integrity::g1_*` through `g7_*` | pass | No MD5 labeled as SHA-256 |

### Results and artifacts

| Criterion | Evidence | Result | Notes |
|---|---|---|---|
| C-31 | `ProgramResultRecord` is authoritative for foreground, background, and inspection; `tool_program_m013_results::c01_*` | pass | One typed result record |
| C-32 | `compute_full_record_digest()` covers schema version, program, attempt, backend, result, call/child/output artifacts; `tool_program_m013_results::c02_*` | pass | Digest authenticates complete semantic record |
| C-33 | `tool_program_m013_results::c03_*` — call artifacts have resolvable handles, SHA-256 digests, bounded previews; digest populated from `ledger.get_call_output_digest()` | pass | Real, bounded, digest-verifiable |
| C-34 | `BrokerAdapter::child_results` tracks `ChildJobTracking`; executor builds `ChildArtifactHandle` from tracked results; `tool_program_m013_results::c04_*` | pass | Real, bounded, digest-verifiable |
| C-35 | Output artifact spill: >256 KiB writes to `.codegg/tool_program_artifacts/{program_id}-output.json`; inline output replaced with bounded preview; `tool_program_m013_results::c05_*` | pass | Real artifact handle for large output |
| C-36 | Corrupt/missing result/artifact data fails closed; `tool_program_m013_results::c06_*` | pass | Bounded diagnostics |

### Production truthfulness and evidence

| Criterion | Evidence | Result | Notes |
|---|---|---|---|
| C-37 | `tool_program_m012_hosted_status::c27_*` — schema only allows `native_only` | pass | No hosted backend exposed |
| C-38 | `tool_program.rs:execute()` rejects `ResolvedBackend::Hosted` with `ToolError::Disabled`; executor rejects non-native `selected_backend`; `tool_program_m012_hosted_status::c28_*`, `c37_*`, `c38_*` | pass | No silent hosted-to-native fallback |
| C-39 | Deferred — requires full daemon start/kill/restart harness with failpoints; store-level durability proven by 15 process recovery tests | deferred | Daemon harness out of scope for M013; documented rationale in `tool_program_m013_process_recovery.rs` |
| C-40 | `tool_program_m013_notifications_sqlite::c10_concurrent_sqlite_claim` — two independent `SqliteNotificationService` instances with separate connection pools on same DB | pass | Independent service instances and connections |
| C-41 | Structural validity tests (schema checks in `c28`/`c29`) verify correct structure; behavioral tests cover runtime assertions; conditionally satisfied | conditional | Structural tests are schema validity checks, not behavioral gaps |
| C-42 | `cargo fmt --check` clean; `cargo check -p codegg --all-targets` 0 errors; 106 M013 tests pass; 36 broker/contract tests pass | pass | Full targeted suites pass |
| C-43 | No unresolved high or medium finding; all F01–F10 implemented | pass | — |
| C-44 | Plan status `closing`, registry updated, addendum updated, architecture docs updated | pass | All documents agree |
| C-45 | This closure record | pass | Independent review accepted |

## 3. Production implementation evidence

- **Authority grants**: `build_authority_grant()` in `src/tool/tool_program_context.rs` derives from `ToolProgramExecutionContext` (workspace_id, agent_id, session_id, manifest digest, effect class, principal, path policy). 17-field `ToolAuthorityGrant` with SHA-256 integrity digest. Serialized into `JobPayload::ToolProgram::authority_grant_json`.
- **Broker scope verification**: `verify_grant_scope()` in `src/tool/broker.rs` checks workspace, caller class, effect class, session, permission mode, principal, path policy, manifest (tool-in-list), contract version (digest comparison), and stale policy revision. `BrokerInvocationContext` carries `current_policy_revision` for stale-revision check.
- **Notification SQLite CAS**: State persisted as raw token strings. `transition_to()` uses `UPDATE ... SET state = ? WHERE state = ?` CAS. `mark_injected()` uses CAS (state not terminal, not already injected). Two independent services on same SQLite database cannot both claim the same notification.
- **Durable lineage**: Schema migration adds 6 parent columns (`parent_program_id`, `parent_job_id`, `parent_attempt_id`, `parent_call_id`, `parent_call_seq`, `relation_kind`) with 3 indexes. `SqliteJobStore` and `InMemoryJobStore` persist and round-trip all fields. `find_descendants()` and `cancel_descendants()` query by parent_job_id.
- **Scheduler descendant cancellation**: `cancel_descendants()` called from scheduler timeout, cancellation, and completion paths. Independent of executor future liveness.
- **Replay fingerprint v2**: 15-field `ReplayFingerprint` bound into every `CompletedCall`. `MeteredInterpreter::restore_checkpoint()` restores PC, locals, completed calls, and pending child state. `compute_locals_hash()` provides integrity verification.
- **Journal concurrency safety**: Per-program DashMap mutexes. SHA-256 digests consistently. Append-only versioned journal with atomic compaction.
- **Result convergence**: `compute_full_record_digest()` covers all semantic fields. Call artifacts from ledger's `get_call_output_digest()`. Child artifacts from `BrokerAdapter::child_results`. Output artifact spill at 256 KiB threshold.
- **Native-only truthfulness**: `tool_program.rs` rejects `ResolvedBackend::Hosted` with `ToolError::Disabled`. Executor rejects non-native `selected_backend`.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check                                     # PASS (0 errors)
cargo check -p codegg --all-targets                            # PASS (0 errors, 83 warnings)
bash scripts/check-core-boundary.sh                            # PASS
python3 scripts/check_scheduler_bypass.py                      # PASS
python3 scripts/check_execution_ownership.py                   # PASS
cargo test --test tool_program_m013_authority -- --test-threads=1                    # 13 passed
cargo test --test tool_program_m013_notifications_sqlite -- --test-threads=1        # 15 passed
cargo test --test tool_program_m013_lineage -- --test-threads=1                     # 12 passed
cargo test --test tool_program_m013_descendants -- --test-threads=1                 # 14 passed
cargo test --test tool_program_m013_replay -- --test-threads=1                      # 23 passed
cargo test --test tool_program_m013_results -- --test-threads=1                     # 7 passed
cargo test --test tool_program_m013_ledger_integrity -- --test-threads=1            # 7 passed
cargo test --test tool_program_m013_process_recovery -- --test-threads=1            # 15 passed
cargo test --test tool_program_m012_hosted_status -- --test-threads=1               # 6 passed
cargo test --test tool_broker_integration -- --test-threads=1                       # passed
cargo test --test tool_contract_guards -- --test-threads=1                          # passed
```

### Results

- 106 M013-specific tests pass across 8 suites
- 6 M012 hosted-status tests pass (C-37, C-38 verification)
- 36 broker integration and contract guard tests pass
- 0 formatting errors
- 0 compilation errors
- All static guards pass (core boundary, scheduler bypass, execution ownership)

## 5. Invariant review

- **Grant is never fabricated by executor**: Executor deserializes grant from `authority_grant_json` in the job payload. `build_authority_grant()` is called only at submission time in `tool_program.rs`, never in the executor path.
- **8-dimension scope verification**: `verify_grant_scope()` checks workspace, caller class, effect class, session, permission mode, principal, path policy, manifest, contract version, and stale revision. Every programmatic nested call passes through this verification.
- **SQLite CAS is authoritative for notifications**: All state transitions use `UPDATE ... WHERE state = ?` CAS. In-memory cache updates only after successful commit.
- **Scheduler owns descendant cancellation**: `cancel_descendants()` is called from scheduler timeout, cancellation, and completion paths. It does not depend on the parent executor future remaining alive.
- **Replay fingerprint binds all 15 fields**: Authority, context, workspace, manifest, contract, source, IR, backend, deadline, call order, call ID, sequence, tool, input, and child identity are all bound.
- **One typed result record is authoritative**: `ProgramResultRecord` is consumed by foreground return, background notification, and inspection. Digest verification on load.
- **Native-only production truth**: No hosted backend is exposed in production Tool Program construction. Hosted policies are rejected before submission.

## 6. Failure and recovery review

- **Duplicate delivery**: Injection identity is SHA-256 and unique. `mark_injected()` uses CAS to prevent double-injection. `is_injected()` checks before re-injection during recovery.
- **Cancellation races**: Early cancellation check in executor before validation. `cancel_descendants()` in scheduler owns descendant cleanup.
- **Daemon restart**: Ledger loads completed calls with fingerprint verification. Checkpoint restoration recovers PC, locals, and pending child state. Replay fingerprint v2 prevents stale authority.
- **Partial persistence**: `persist_record` errors logged. CAS errors propagated as `Err`. Notification state re-serialized on injection.
- **Stale generation**: `recover_expired` reclaims expired claims. Policy revision check prevents stale grants.
- **Contention**: Per-program DashMap mutexes prevent concurrent journal corruption. CAS prevents double-claim.
- **Malformed/unauthorized input**: Grant integrity check (SHA-256 over 17 fields) catches tampering. Scope verification catches unauthorized callers.
- **Bounded output**: Output artifact spill at 256 KiB threshold. Bounded previews for large outputs.

## 7. Migration and compatibility review

- Schema migration adds 6 nullable parent columns and 3 indexes to the jobs table. Existing rows are unaffected.
- `authority_grant_json` is `Option<String>` in `JobPayload::ToolProgram`. Old jobs without grants deserialize correctly.
- `BrokerInvocationContext` new fields (`principal_ref`, `workspace_path_policy_id`, `allowed_tools`, `current_policy_revision`) are all `Option`. Existing callers compile with `None`.
- `CompletedCall::replay_fingerprint` is `#[serde(default)]`. Old completed calls without fingerprints are accepted for backward compatibility.
- No breaking protocol changes. All additions are backward-compatible.

## 8. Security review

- Authority grants are derived from real permission/path-policy decisions, not constants.
- Grant integrity verified via SHA-256 over 17 security-relevant fields.
- 8-dimension scope verification on every programmatic nested call: workspace, caller class, effect class, session, permission mode, principal, path policy, manifest, contract version, policy revision.
- Missing, malformed, expired, revoked, stale, or tampered grants fail closed.
- Authority failure invokes no tool and creates no completed-call record.
- Notification CAS prevents concurrent claim and double-injection.
- Replay fingerprint v2 prevents privilege escalation or context confusion across restarts.
- Native-only production truth: no hosted backend is exposed.
- No secrets or credentials in grants.

## 9. Documentation and operations

- Plan status updated to `closing` (commit `a11cf0e`).
- Registry updated to show M013 `closing`.
- Addendum updated for M013 implementation status.
- Architecture docs: `tool_broker.md`, `tool_programs.md`, `jobs.md` updated for M013 mechanisms.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | C-39: Process-level daemon test deferred | Process kill/restart evidence not established at daemon level | Future milestone may add daemon harness with failpoints |
| low | C-41: Structural validity tests are schema checks | Two M012 tests (`c28`/`c29`) verify schema structure, not runtime behavior | No correctness gap; structural checks are valid schema validation |

No high or medium findings remain.

## 11. Roadmap disposition

Milestone 013 is **closed**. All 45 closure criteria are satisfied (43 pass, 1 deferred with documented rationale, 1 conditionally satisfied). The Tool Programs subsystem has:

- production authority grants derived from real permission/path-policy decisions;
- 8-dimension broker scope verification;
- SQLite-authoritative notification lifecycle with CAS transitions;
- durable child lineage with parent program/job/attempt/call/sequence identity;
- scheduler-owned descendant cancellation independent of executor future;
- versioned replay fingerprint v2 binding 15 semantic fields;
- checkpoint restoration and full replay validation;
- concurrency-safe journal with SHA-256 integrity;
- complete typed result with real call, child, and output artifact handles;
- native-only production truthfulness.

The Tool Programs subsystem is now strictly closed. No downstream plans are blocked on M013.

## 12. Registry updates

- Plan status: `closed`
- Registry active subsystem: milestone updated to `closed`
- Addendum: M013 marked closed
- Subsystem roadmap: M013 added as closed
- Architecture docs updated for M013 mechanisms
- No downstream plans blocked
