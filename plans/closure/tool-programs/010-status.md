# Tool Programs Milestone 010 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tool-programs/010-harness-eggpool-chaos-performance-and-closure.md`

Source subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-10--harness-eggpool-chaos-performance-and-closure`

Repository baseline reviewed: `64c6d5ac`; implementation commit:
`2f5e3d3dc9c057f925d59625b50a6b1eaae3a3dd`

Implementation commits or pull requests:

- Implementation commit: `2f5e3d3dc9c057f925d59625b50a6b1eaae3a3dd`
- Closure/reconciliation commit: recorded at the closure commit below

## 1. Executive finding

The native M010 capability is implemented and usable through a non-TUI
production path. A real `core-stdio` daemon process now boots an isolated
durable catalog, registers the Tool Program scheduler executor, accepts an
immutable source reference, executes through the scheduler and Tool Broker,
and exposes bounded public inspection including a redacted call ledger.

Deterministic scenario, chaos, resource, model-behavior, runtime, and fault
tests pass. The plan is conditionally closed because no local Eggpool
credentials/endpoint or ACP adapter was available in this environment, and
the broad workspace test PTY did not return a trustworthy completion status.
Those limitations are recorded rather than presented as evidence.

No unresolved high- or medium-severity correctness or security finding was
identified. The remaining conditions are operational/evidence limitations.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Native/headless production path | `scripts/e2e/tool_program_harness.py --mode native`; isolated `core-stdio` process | pass | Uses durable catalog, scheduler, executor, JobStore, and public protocol |
| Submitted source is immutable and verified | `src/tool/tool_program_source.rs`; runtime tests | pass | SHA-256, length, symlink, traversal, atomic-write checks |
| Tool Program executor is scheduler-owned | `src/scheduler/scheduler.rs`; native harness | pass | Default registry now includes `ToolProgramExecutor` |
| Public list/inspect/call-page path | `src/core/daemon.rs`; native harness | pass | No private interpreter-memory assertions in native closure path |
| Bounded redacted call ledger | `src/tool/tool_program_ledger.rs`; 2 unit tests; native call-page assertion | pass | Raw args/results are never persisted or projected |
| Frozen allowed-tool manifest | `JobPayload::ToolProgram`; broker adapter enforcement; native `read` call | pass | Calls outside the manifest fail closed |
| Deterministic scenario corpus | `tests/tool_program_scenarios.rs` | pass | 13 tests, explicit Tokio flavor |
| Mixed fault injection at or above 10% | `tests/tool_program_chaos.rs` | pass | Seeded 10%, 30%, and 50% runs; seeds 42, 123, 999 |
| Resource/convergence evidence | `tests/tool_program_resource_convergence.rs`; native job convergence | pass | 10 fixture tests plus real job/attempt/ledger convergence |
| Direct/programmatic correctness and context metric | `tests/tool_program_model_behavior.rs` | pass | 14 scripted tests; paired route fixture checks call equality and transcript-byte reduction |
| Model behavior bounds | `tests/tool_program_model_behavior.rs` | pass | Invalid-source correction is bounded; background path does not poll |
| Exact Eggpool `mimo-v2.5` and no fallback | Eggpool CLI gate in harness | partial | Code requires explicit connection ID, exact identity, no fallback, and a bounded behavior request; live run skipped because credentials were absent |
| ACP adapter corpus | Harness ACP mode | not run | Correctly reported skipped; ACP adapter is not scheduled |
| Security/static guards | core, scheduler, execution, projection, websocket, cwd, discovery, catalog, provider guards | pass | New M010 paths pass; unrelated baseline agent/Tokio audits remain stale |
| Full workspace CI evidence | CI workflow / broad local matrix | not confirmed | The capped local workspace PTY did not return a reliable completion status |

Milestone status reconciliation: M001–M009 are closed by their accepted
records. M006's historical closure record was corrected from `closing` to
`closed` so it agrees with the registry and roadmap. M010 is conditionally
closed pending the named operational evidence.

## 3. Production implementation evidence

- `src/tool/tool_program_source.rs` provides the workspace-local immutable
  source store and digest/length verification.
- `src/tool/tool_program_ledger.rs` provides atomic bounded ledger writes,
  shape-only redaction, pagination, identity validation, and symlink/path
  checks.
- `src/tool/tool_program.rs` persists source before scheduler submission and
  freezes the allowed-tool manifest in the job payload.
- `src/scheduler/tool_program_executor.rs` loads the submitted source,
  verifies the source and IR, applies the manifest at the broker boundary,
  executes under scheduler cancellation/deadline controls, persists the call
  ledger, and receives the daemon-owned submission facade for child jobs.
- `src/scheduler/scheduler.rs` registers the Tool Program executor in the
  daemon's default executor set.
- `src/core/daemon.rs` implements bounded Tool Program list/inspect/page
  responses over durable jobs, attempts, workspace services, and ledger data.
- `src/main.rs` keeps `core-stdio` stdout protocol-clean, initializes a
  migrated durable catalog, and supports an isolated catalog override for
  the native harness.
- `scripts/e2e/tool_program_harness.py` is a client of CodeGG; it does not
  instantiate a second interpreter or bypass scheduler admission.
- Architecture and reusable skill documentation cover source storage,
  inspection, Eggpool secret handling, native mode, and ACP skip semantics.

## 4. Verification executed

### Commands run locally

```text
cargo fmt --all -- --check                                      PASS
cargo check -p codegg --all-targets                             PASS (0 errors; existing warnings)
cargo test -p codegg --lib tool::tool_program_ledger           PASS (2)
cargo test -p codegg --test tool_program_runtime --quiet        PASS (10)
cargo test -p codegg --test tool_program_fault_injection --quiet PASS (38)
cargo test -p codegg --test python_scheduler_execution --quiet  PASS (10)
cargo test -p codegg --test tool_program_scenarios --quiet      PASS (13)
cargo test -p codegg --test tool_program_chaos --quiet          PASS (13)
cargo test -p codegg --test tool_program_resource_convergence --quiet PASS (10)
cargo test -p codegg --test tool_program_model_behavior --quiet  PASS (14)
python3 scripts/e2e/tool_program_harness.py --mode native       PASS (1/1)
python3 scripts/e2e/tool_program_harness.py --mode scripted --scenario all PASS (13/13; 20.6s)
python3 scripts/e2e/tool_program_harness.py --mode eggpool --model mimo-v2.5 --no-model-fallback SKIP (no URL/key)
```

Static/operational guards run locally:

```text
check-core-boundary.sh                              PASS
check_daemon_cwd_usage.py                           PASS
check_scheduler_bypass.py                           PASS
check_execution_ownership.py                        PASS
check_git_forbidden_patterns.py                     PASS
check_projection_disclosure.sh                      PASS
check_projection_transport_isolation.py             PASS
check_websocket_bounds.py                            PASS
check_project_agent_pwd_inference.py                PASS
check_discovery_invariants.py                        PASS (5/5)
check_project_catalog_invariants.py                  PASS (7/7 after v33 guard correction)
check_provider_connections_m4_coverage.sh            PASS
check_provider_connections_tombstone_compat.sh       PASS
```

Known baseline verification limitations:

- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  stops on seven existing projection/provider warnings outside the M010
  change set.
- `check_builtin_agents.py` reports two pre-existing generated-agent prompt
  mismatches (`explore`, `general`).
- `check-tokio-test-flavors.py` reports the repository's existing bare-test
  inventory; all new M010 tests use explicit `current_thread` flavor.
- The capped workspace-wide all-feature test PTY did not return a reliable
  completion status. It is not claimed as pass or fail and remains CI-owned
  evidence.

All seeds in the M010 deterministic corpus are recorded in the scenario
fixtures; the 10% mixed-fault gate uses seed 42 and the higher-rate probes
use seeds 123 and 999. Focused tests run once each with one test thread;
the scripted harness runs one repetition of each listed suite.

## 5. Invariant review

- One logical program identity is retained from source digest through job,
  inspection, and ledger filename.
- Scheduler admission is the only production durable execution authority.
- Native execution uses the daemon JobStore, workspace lease, scheduler,
  executor, Tool Broker, and protocol inspection boundary.
- Source, IR, and manifest are verified before interpretation.
- Completed call summaries are bounded and redacted; raw source and call
  bodies are excluded from public DTOs.
- Runtime bounds, cancellation, deadlines, static syntax validation, and
  broker manifest checks remain active under fault injection.
- Native closure assertions use public protocol state; private state is used
  only in focused unit fixtures.

## 6. Failure and recovery review

The deterministic suites cover transient broker faults, malformed output,
rate limiting, cancellation, budget exhaustion, restart/replay fixtures,
notification duplication, contention, and terminal/recoverable convergence.
The native process path additionally proves catalog migration, scheduler
executor registration, job/attempt completion, public inspection, and call
ledger pagination after execution.

Source persistence failure, digest mismatch, invalid path/symlink, missing
source reference, IR mismatch, manifest mismatch, and ledger-write failure
are typed fail-closed executor outcomes. Atomic source/ledger writes avoid
partially published records. The broad process/permit/lease leak matrix was
not confirmed locally and is not claimed here.

## 7. Migration and compatibility review

Tool Program payload additions are serde-defaulted and additive for old
records; old jobs without durable source references fail closed at executor
admission rather than executing an unverified fixture. The native core-stdio
path uses the durable catalog schema and a single-connection migration
bootstrap before reopening the normal pool. Public inspection fields remain
bounded and frontend-neutral. ACP and hosted provider behavior are not
silently claimed by native evidence.

## 8. Security review

- Eggpool URL, API key, provider response bodies, and source/result bodies are
  not printed or committed by the harness.
- Live mode requires `--no-model-fallback`, exact `mimo-v2.5` identity, and an
  explicit `CODEGG_EGGPOOL_CONNECTION_ID`.
- Source and ledger identities reject traversal, absolute paths, symlinks, and
  malformed program IDs.
- Program calls are checked against the frozen allowed-tool manifest before
  broker execution.
- Public inspection returns hashes and bounded summaries, not raw source,
  arguments, or output bodies.
- Static authority, workspace-CWD, scheduler-bypass, execution-ownership,
  projection-disclosure, and secret-boundary guards pass.

## 9. Documentation and operations

Updated architecture and reusable skill documentation cover deterministic,
native, Eggpool, ACP, source/ledger storage, secret handling, bounded
inspection, and closure evidence. Operators should run native/scripted mode
without credentials and supply the three Eggpool variables only for a live
operational run. CI must run the repository's full all-feature test matrix
and stale generated-agent/Tokio guards separately from M010's focused gate.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Eggpool endpoint/key and ACP adapter unavailable locally | Live provider/ACP evidence cannot be recorded in this environment | Run exact Eggpool command with explicit connection ID when supplied; run ACP mode when adapter is scheduled |
| low | Workspace-wide all-feature test PTY did not return a trustworthy completion status | Local full-suite result is unknown; focused/native evidence remains valid | Require CI all-feature matrix before strict subsystem closure |
| low | Existing clippy, generated-agent, and bare-Tokio guard debt | Repository-wide clean gate is not currently available | Track in owning maintenance plans; do not attribute to M010 |

No high or medium findings remain.

## 11. Roadmap disposition

M010 is conditionally closed: the native capability and deterministic
closure evidence are complete, while live Eggpool, ACP, and CI-owned evidence
remain explicitly outstanding. The subsystem roadmap remains active/closing
rather than claiming a strict all-provider closure.

## 12. Registry updates

- Move M010 from the dependency-ready table to recently closed as
  `conditionally closed`.
- Mark the Tool Programs roadmap current milestone as M010 conditionally
  closed and retain `active`/`closing` status because operational evidence is
  outstanding.
- No registered downstream plan lists M010 as a dependency; the blocked-work
  section remains empty. No future plan is newly unblocked by this closure.
- Keep M009 closed and do not claim hosted equivalence beyond its own accepted
  deterministic evidence record.
