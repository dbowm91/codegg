# Post-Audit Correctness, Simplification, and Footprint Milestone 006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-correctness-simplification/006-test-stack-and-resource-root-cause-correction.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Implementation commit:

- `a4402db` — correct daemon socket test stack usage

## 1. Executive finding

M006 is complete and closed. The historical daemon-socket stack overflow was
reproduced without `RUST_MIN_STACK`, localized to the socket adapter's direct
await of the monolithic daemon request-dispatch future, and corrected by
boxing that future at the socket ownership boundary. The global 32 MiB stack
override was removed from CI, local verification, and active testing
documentation. The full capped workspace suite passes with the environment
unset.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Identify the original reproducer | `env -u RUST_MIN_STACK CARGO_BUILD_JOBS=1 cargo test -p codegg --lib core::transport::daemon_socket -- --test-threads=1 --nocapture` | pass | Before the fix, `socket_consecutive_subscriptions_yield_distinct_identities_and_isolation` aborted with stack overflow; a focused `socket_f0_successful_production_write_is_observed` run also localized failure to request handling. |
| Determine the root cause | `src/core/transport/daemon_socket.rs` inspection and instrumented focused run | pass | `CoreDaemon::handle_request_with_client` is a full-protocol async dispatch future; the overflow occurred while awaiting it after the projection request was sent, before response handling completed. |
| Apply the smallest correction | `handle_request_for_client_bounded` in `daemon_socket.rs` | pass | The helper heap-boxes the existing future for socket requests and cleanup calls; no authority, protocol, or lifecycle behavior changed. |
| Reproducer passes with normal stack | Exact minimal test command with `RUST_MIN_STACK` unset | pass | 1 passed. |
| Related daemon/projection tests pass | `core::transport::daemon_socket` and `single_daemon_lifecycle` commands | pass | 33 daemon-socket tests and 5 lifecycle tests passed. |
| Remove the global workaround | `.github/workflows/ci.yml`, `scripts/verify.sh`, `architecture/testing.md`, and `AGENTS.md` | pass | No routine CI or local verification path exports the 32 MiB bound. |
| Preserve bounded verification posture | Full workspace command with `CARGO_BUILD_JOBS=1` and one test thread | pass | No new lane, scanner, benchmark, profiler, or stack guard was added. |

## 3. Production implementation evidence

- `src/core/transport/daemon_socket.rs` now uses one private helper that
  boxes the daemon's large request-dispatch future before polling it from the
  connection task. The same boundary is used by projection subscription
  rollback and disconnect cleanup paths.
- `.github/workflows/ci.yml` and `scripts/verify.sh` no longer define or
  export `RUST_MIN_STACK`; Cargo build jobs and serial test threads remain
  bounded.
- `architecture/testing.md` and `AGENTS.md` describe the resulting resource
  contract without a global stack requirement.
- The existing projection identity/isolation and daemon lifecycle coverage was
  retained unchanged.

No storage, protocol, migration, scheduler authority, or user-facing daemon
behavior changed.

## 4. Verification executed

### Commands run

```bash
# Pre-fix reproduction
env -u RUST_MIN_STACK CARGO_BUILD_JOBS=1 cargo test -p codegg --lib core::transport::daemon_socket -- --test-threads=1 --nocapture

# Focused post-fix evidence
env -u RUST_MIN_STACK CARGO_BUILD_JOBS=1 cargo test -p codegg --lib core::transport::daemon_socket::daemon_socket_integration_tests::socket_consecutive_subscriptions_yield_distinct_identities_and_isolation -- --test-threads=1 --nocapture
env -u RUST_MIN_STACK CARGO_BUILD_JOBS=1 cargo test -p codegg --lib core::transport::daemon_socket -- --test-threads=1 --nocapture
env -u RUST_MIN_STACK CARGO_BUILD_JOBS=1 cargo test --test single_daemon_lifecycle -- --test-threads=1 --nocapture

# Repository verification
env -u RUST_MIN_STACK scripts/verify.sh quick
env -u RUST_MIN_STACK CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
```

### Results

- The pre-fix focused run reproduced `SIGABRT` stack overflow in the daemon
  socket test binary with the variable unset.
- The exact reproducer passed after the correction: 1 passed.
- The complete daemon-socket module passed: 33 passed, 0 failed.
- `single_daemon_lifecycle` passed: 5 passed, 0 failed.
- `scripts/verify.sh quick` passed, including formatting, generated-agent
  checks, boundary/static guards, and workspace all-target checking.
- The capped workspace suite passed: 9,686 passed, 10 ignored across 189
  suites in 544.13 seconds.

No hosted result is claimed here; the equivalent broad workspace command was
run locally with the same bounded Cargo/test-thread policy and no stack
override.

## 5. Invariant review

- Daemon/client transport wire behavior is unchanged; only future placement
  changed.
- Projection subscription ownership, replay/live handoff, cancellation, and
  cleanup paths remain covered by the existing 33-test transport module.
- Singleton daemon lifecycle behavior remains covered by all five lifecycle
  tests.
- No production thread or process stack size was increased.
- Routine verification remains one CI job with bounded build/test concurrency.
- No test sleeps, races, broad transport rewrite, or static stack scanner was
  introduced.

## 6. Failure and recovery review

The boxed future does not alter request ordering, cancellation, or error
mapping. A request failure still becomes the existing typed handler error
response; projection setup failures still run the existing rollback and
disconnect cleanup paths. The full transport and workspace suites provide
evidence that subscriptions and daemon resources converge normally under
success, peer-close, cancellation, replay, and cleanup cases.

## 7. Migration and compatibility review

No storage migration, configuration migration, protocol negotiation, or user
action is required. The removed environment variable was a test/CI workaround,
not a persisted or supported runtime setting. The socket wire format and
daemon endpoint contract are unchanged.

## 8. Security review

No authorization, path, secret, process-spawn, sandbox, or network policy was
changed. The boxed dispatch preserves the existing daemon-owned request
authority and client identity passed to every request and cleanup operation.

## 9. Documentation and operations

Updated:

- `.github/workflows/ci.yml`
- `scripts/verify.sh`
- `architecture/testing.md`
- `AGENTS.md`
- `plans/implementation/post-audit-correctness-simplification/006-test-stack-and-resource-root-cause-correction.md`
- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- `plans/registry.md`

No new static guard or operational resource requirement was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No M006 correctness, security, migration, or resource finding remains. | None | None |

## 11. Roadmap disposition

M006 is closed. M007 has no dependency on M006 and remains `ready`. M008 is
not unblocked: its remaining hard dependency is M007, so it remains `blocked`
in both the roadmap and registry. No other registered blocked plan names M006
as a dependency, and no new corrective plan is required.

The independent runtime-safety Landlock evidence condition remains unchanged
and is not part of this workstream.

## 12. Registry updates

- M006 is removed from dependency-ready implementation plans and recorded as
  closed under recently closed implementation plans.
- The M006 implementation plan is marked implemented and links to this record.
- The post-audit roadmap marks M006 closed, retains M007 as ready, and keeps
  M008 blocked only on M007.
- The registry's active subsystem row now reports M007 ready and M008 blocked
  on M007.
