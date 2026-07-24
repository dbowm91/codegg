# Tool Programs Milestone 011 — Closure Record

Status: closed

## Scope and closure authority

Milestone 011 (production correctness and ownership closure) is accepted for the
native restricted-Python production path. The implementation was reviewed at
`0ae10673eb2d1264909977abf694d6d96fbcac9d9`:

`feat(tool-programs): close production ownership boundaries`

The closure/governance reconciliation was committed at `705ae2cd`:

`docs(plans): close Tool Programs M011`

The authoritative documents are:

- implementation plan: `plans/implementation/tool-programs/011-production-correctness-and-ownership-closure.md`;
- corrective addendum: `plans/subsystems/tool-programs-correctness-closure-addendum.md`;
- subsystem roadmap: `plans/subsystems/tool-programs-roadmap.md`;
- applicable ADR: `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`.

This record is the closure authority for M011. Historical M001–M010 closure
records remain unchanged traceability artifacts; they are not rewritten to
conceal the findings that M011 corrected.

Strict native closure has no unresolved high or medium correctness, security,
recovery, identity, authority, or resource-ownership finding. Hosted capability
selection is wired through normal policy resolution and fails closed when no
hosted transport is attached. No live external hosted-provider, Eggpool, or ACP
run is claimed here; that is an operational evidence condition, not a native
correctness dependency.

## Finding-to-evidence matrix

| Finding | Disposition | Evidence |
|---|---|---|
| F-01 source digest used as invocation identity | corrected | Explicit invocation key and generated program identity are persisted separately from source/IR digests; retry keys are rebuilt from durable jobs and deliberate identical source submissions do not alias. `tests/tool_program_m011_correctness.rs`, `src/scheduler/submission.rs`, `src/tool/tool_program.rs`. |
| F-02 parent lineage and authority dropped | corrected | `ToolProgramExecutionContext` persists workspace, session, turn, agent, parent lineage, principal/authority references, path-policy identity, policy revision, provider/backend policy, and correlation; context and authority digests are validated before execution. `crates/codegg-core/src/jobs/mod.rs`, `src/tool/tool_program_context.rs`. |
| F-03 Broker was not the enforced production boundary | corrected | AgentLoop direct calls and program calls use `ToolBroker`; typed authority, caller/effect policy, path/cwd, JSON schemas, bounds, deadline/cancellation, and artifact handles are enforced at dispatch. `src/agent/loop.rs`, `src/tool/broker.rs`, `scripts/check_tool_broker_boundary.py`. |
| F-04 call replay was not durable at call boundaries | corrected | Atomic reservation/completion/checkpoint journal records are written before interpreter advancement; durable replay verifies call identity, arguments, context, and control flow and reports divergence. `src/tool/tool_program_ledger.rs`, `crates/codegg-core/src/tool_program/interpreter.rs`, `tests/tool_program_fault_injection.rs`. |
| F-05 timeout and heartbeat were not scheduler-owned end to end | corrected | Effective deadlines are persisted, scheduler execution has an outer timeout, nested deadlines narrow, and interpreter/child progress updates the attempt heartbeat. `src/scheduler/scheduler.rs`, `src/scheduler/tool_program_executor.rs`, `crates/codegg-core/src/jobs/mod.rs`. |
| F-06 notifications were not durably parent-addressed | corrected | Terminal-only notification creation, real session/turn/agent lineage, SQLite-backed state, claim leases, acknowledgement, expiry/recovery, and bounded handles are production-wired. `src/scheduler/tool_program_notifications.rs`, `crates/codegg-core/src/session/schema.rs`, `src/agent/loop.rs`. |
| F-07 child identity/cancellation/deadline/resources/artifacts incomplete | corrected | Child sequence identity and replay keys include program/attempt/call lineage; source is delegated, cancellation/deadline are inherited, and child results/artifacts are retained through the broker and scheduler paths. `crates/codegg-core/src/tool_program/child_job.rs`, `src/scheduler/tool_program_executor.rs`. |
| F-08 foreground result mapping was lossy | corrected | `ProgramResultRecord` is the typed source for foreground, background, inspection, and notification projections; counters and terminal/failure classes are not parsed from summaries. `src/tool/tool_program_result.rs`, `src/tool/tool_program.rs`. |
| F-09 hosted support was not selected by normal runtime policy | corrected at the policy boundary | `HostedBackendPolicy` is persisted and resolved against provider capability in the normal tool-program path; `HostedRequired` fails closed without a transport and `HostedPreferred` logs explicit native fallback. Provider policy tests pass. A live external transport run is not claimed. `crates/codegg-providers/src/responses_api.rs`, `src/tool/tool_program.rs`. |
| F-10 evidence and registry truth were inconsistent | corrected | M011 contract tests, migrations, static guards, architecture updates, this closure record, roadmap/addendum status, and registry are reconciled. Process-level daemon failures and repository-wide baseline gates are documented below rather than misattributed. |

## Acceptance and ownership matrix

| Acceptance area | Result | Evidence |
|---|---|---|
| Identity and lineage | pass | Durable execution context, invocation-key retry identity, source/IR separation, and typed authority references are present and validated. |
| Broker and authority | pass | Direct and programmatic calls share `ToolBroker`; direct-only and mutating tools remain unavailable to the programmable palette. |
| Recovery and bounded execution | pass | Call journals, typed result store, checkpoints, scheduler deadlines, heartbeats, cancellation propagation, and terminal persistence are wired. |
| Child jobs | pass | Call-identity-based child submission, narrowed deadline/cancellation, scheduler-owned child execution, and bounded artifact ownership are wired. |
| Notifications | pass | Terminal SQLite records use real parent session identity and claim/ack/recovery state; AgentLoop injection acknowledges only after inclusion. |
| Results, artifacts, projections | pass | Atomic file-backed artifact/result records and bounded handles are used; semantic counters come from typed records. |
| Hosted integration | pass for deterministic policy semantics | Normal policy resolution, capability negotiation, explicit fallback, and fail-closed required mode are tested. Live provider execution is operational follow-up only. |
| Governance and security | pass for M011-owned guards | Migration, broker-boundary, core-boundary, scheduler-bypass, execution-ownership, daemon-cwd, discovery, and catalog checks pass; unrelated repository-wide lint/flavor baseline failures are recorded below. |

## Migration and compatibility

- The schema change is additive: the session schema moved to v34 with the
  `tool_program_notification` table and claim/delivery indexes.
- Legacy payloads without a valid execution context, source reference, or
  authority digest fail closed and remain inspectable; they are not merged into
  new invocation identities.
- Existing notification and job records retain their original IDs. Durable
  retry-index reconstruction scans persisted Tool Program jobs after restart.
- `cargo test --test storage_migrations`: 4 passed.
- `cargo test -p codegg-core session -- --test-threads=14`: 44 passed.

## Security, ownership, and redaction review

- The programmable palette remains read-only. Mutation, shell, patch, Git
  mutation, commit, push, approval-sensitive, and subagent operations remain
  direct-only.
- No boolean authorization bypass remains on the program path; broker calls
  carry typed authority and policy references.
- Input/output schemas, path containment, workspace identity, deadlines,
  cancellation, output bounds, artifact handles, and provenance are checked at
  the canonical broker boundary.
- Raw credentials, unbounded output, raw source/arguments, and hidden reasoning
  are excluded from parent transcript/public notification payloads.
- `scripts/check_tool_broker_boundary.py`: passed.
- `scripts/check-core-boundary.sh`: passed.
- `scripts/check_scheduler_bypass.py`: passed.
- `scripts/check_execution_ownership.py`: passed.
- `scripts/check_daemon_cwd_usage.py`: passed.
- `scripts/check_project_catalog_invariants.py`: 7/7 passed.
- `scripts/check_discovery_invariants.py`: 5/5 passed.

## Verification record

Environment: macOS, Rust workspace, repository branch
`agent/tool-program-m010-closure`, implementation commit
`0ae10673eb2d1264909977abf694d6d96fbcac9d9`. Commands were run with the
repository RTK command wrapper and test concurrency was capped where relevant.

Passing M011-focused evidence:

- `cargo fmt --all -- --check` — passed.
- `cargo check -p codegg --all-targets` — passed.
- `cargo check --workspace --all-targets --all-features` — passed with no errors.
- Tool Program matrix covering broker integration, build/test matrix, child
  recovery, context artifacts, fault injection, lifecycle, notifications,
  read palette, recovery, runtime, and storage migrations — 193 passed across
  11 suites.
- Background and M011 contract suites — 14 passed across 2 suites.
- `cargo test -p codegg --lib tool::broker` — 4 passed.
- `cargo test -p codegg-providers backend_policy -- --test-threads=14` — 2
  passed.
- `git diff --check` — passed before the implementation commit.

The implementation also retains the broader hosted adapter, contention,
equivalence, recovery, and security test suites; their deterministic policy and
normalization coverage is distinct from a live provider call.

Known repository-wide baseline gates that do not touch or invalidate M011:

- `cargo clippy -p codegg --all-targets -- -D warnings` still reports six
  pre-existing errors in untouched `crates/codegg-core/src/projection_replay/`
  files (`artifact_registry.rs`, `artifacts.rs`, `context.rs`, `redactor.rs`,
  and `seam.rs`); no M011 warning was reported.
- `python3 scripts/check-tokio-test-flavors.py` still reports the repository's
  pre-existing 1,062 bare `#[tokio::test]` annotations. The new M011 async tests
  use an explicit runtime flavor.
- The capped full workspace all-features test reached compilation and then
  hit a stack overflow in the unchanged
  `core::transport::daemon_socket::daemon_socket_integration_tests::socket_f1_peer_closes_before_canonical_response_returns_io_error`
  test. Running that test alone with one test thread reproduced the same
  failure; it is outside the Tool Programs path.

These are recorded baseline repository conditions, not M011 findings. No
M011-owned targeted test or static ownership guard failed.

## Remaining operational evidence

The native correctness closure does not require an external service. A live
OpenAI-compatible hosted transport, Eggpool run, and ACP end-to-end run were
not available in this review environment. The implementation therefore makes
the boundary explicit: native remains the safe default, hosted selection is
capability- and policy-gated, `HostedRequired` fails closed when transport is
unavailable, and `HostedPreferred` produces an observable fallback. No live
hosted behavior is represented as deterministic closure evidence.

## Dependency and future-plan audit

The registered dependency graph contains M001–M010 → M011 → strict Tool
Programs closure and no registered downstream Tool Programs milestone. The
blocked-work table is empty. M011 therefore unblocks no future registered plan;
there was no `blocked` → `ready` status transition to make. Deferred product
items remain explicitly unregistered in `plans/registry.md`.

## Final disposition

M011 is closed for strict native production correctness and ownership. The
read-only palette, scheduler authority, canonical Broker, durable identity and
replay boundaries, bounded artifacts, child ownership, typed results, and
parent notification lifecycle are now represented in production code and
targeted evidence. The roadmap, addendum, implementation plan, closure record,
and registry agree on this disposition.
