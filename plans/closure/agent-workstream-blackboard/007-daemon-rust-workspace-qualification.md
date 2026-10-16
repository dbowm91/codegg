# Agent Workstream Blackboard M007 — Daemon Rust Workspace Qualification

Status: closed — daemon-backed realistic Rust workspace JobId evidence and exact-head hosted CI passed; M007 per-workspace build admission is strictly qualified

Source implementation plan: `plans/implementation/agent-workstream-blackboard/007-build-resource-coordination.md`
Prior closure record: `plans/closure/agent-workstream-blackboard/007-status.md` (conditional record retained as historical evidence)
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md`
Qualified source SHA: `79cc725b223c8dbed399107f11679424c5eb4e7a` (M007 daemon consumer and exact-head CI)

## 1. Executive finding

The prior medium finding is resolved locally. A real dependency-free Rust
library was submitted through the daemon's `CoreRequest::JobSubmit` as a typed
`Build`, executed by the production scheduler and managed-argv executor, and
awaited through the daemon by its returned JobId. Cargo completed successfully
with `--offline`. The durable JobRecord retained the canonical WorkspaceId and
one workspace build slot through terminal completion.

This evidence exercises the ordinary local isolated workspace path. It adds
no shared Cargo target, compiler cache, build coalescing, or shared checkout.
M007 remains independent of M005. Exact-head hosted CI passed; strict closure
and the registry/roadmap reconciliation are recorded below.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Authorized daemon JobId consumer submits a typed Build | `core::daemon::tests::daemon_build_job_qualifies_real_rust_workspace_and_workspace_quota` | pass | `CoreRequest::JobSubmit` returns a durable JobId; test waits through `CoreRequest::JobWait`. |
| Realistic Rust workspace completes through production executor | Same test, `cargo check --offline` over a local Rust library fixture | pass | No dependencies or public network; production managed process and scheduler path used. |
| Workspace identity and build quota are persisted before and after execution | Same test reads the durable JobRecord before and after completion | pass | `workspace_build_slots == 1`; WorkspaceId remains the fixture workspace. |
| Operational evidence | Captured test output | pass | JobId `fed6872c-0a7c-4f62-a8c0-9400e57cf482`; WorkspaceId `62f78a87-bfc5-493c-ab58-ee38ba110dd1`; status `completed`. |
| Same-workspace bounds, unrelated-workspace independence, cancellation and permit lifetime | Existing scheduler admission, cancellation, restart, and permit-lifecycle tests; prior exact-head CI | pass in deterministic suite | Shared cache and target placement remain excluded. |
| Exact-head hosted verification | CI run `38930928833` on `79cc725b223c8dbed399107f11679424c5eb4e7a` | pass | 13,971/13,971 passed, 6 skipped; formatting, workspace Clippy, guards, sandbox-helper build, and workspace tests passed. |

## 3. Production implementation evidence

The configured `JobSubmissionService` normalizes one workspace build slot from
the typed `JobKind::Build`. The daemon stores the job and its canonical
WorkspaceId, admits it through the existing scheduler, runs the actual Cargo
command through `ManagedArgvExecutor`, and returns the terminal result through
the daemon JobWait projection. The test observes the durable slot request on
both sides of completion.

## 4. Verification executed

Pinned local toolchain: Rust 1.89 aarch64-apple-darwin; target directory
`/tmp/codegg-m006-qual-target`.

```bash
RUSTC=.../1.89-aarch64-apple-darwin/bin/rustc RUSTDOC=.../1.89-aarch64-apple-darwin/bin/rustdoc \
  CARGO_TARGET_DIR=/tmp/codegg-m006-qual-target CARGO_INCREMENTAL=0 \
  rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --lib \
    core::daemon::tests::daemon_build_job_qualifies_real_rust_workspace_and_workspace_quota \
    --locked -- --nocapture
```

Result: one test passed, zero failed. Captured line:

```text
M007 Rust workspace qualification: job_id=fed6872c-0a7c-4f62-a8c0-9400e57cf482 workspace_id=62f78a87-bfc5-493c-ab58-ee38ba110dd1 status=completed
```

The exact-head CI run `38930928833` passed on source SHA
`79cc725b223c8dbed399107f11679424c5eb4e7a`. An intervening run `38927512933`
stopped on an M006 Linux fixture lower-bound assertion (10,494 passed before
the failure); the bound was corrected and the full suite passed on rerun.
Earlier exact-head run `38881759368` on
`50946611fc73fbf485d8137ed1c96cfaab5a7323` passed the then-current scheduler
implementation and 13,969 workspace tests; it predates this daemon
qualification test and is not evidence for the current head.

## 5. Residual limits and findings

| Severity | Finding | Disposition |
|---|---|---|
| none | No known M007 correctness finding remains after daemon and hosted qualification. | Per-workspace admission caps scheduled Build jobs; it does not provide shared Cargo targets, generic inline-writer containment, or M005 shared-checkout authority. |

## 6. Strict closure and downstream audit

M007 is strictly closed on source SHA
`79cc725b223c8dbed399107f11679424c5eb4e7a`: the daemon-backed Rust workspace
consumer completed with durable JobId/workspace quota evidence and exact-head
CI `38930928833` passed 13,971 tests with 6 skipped.

Registry audit: M006 is strictly closed by
`plans/closure/agent-workstream-blackboard/006-newmanaged-materializer-addendum.md`.
M008 is unblocked and active for isolated S0–S3 comparison. S4 remains gated
on M005, which remains active for feasibility only with shared writes
unavailable. No other registered plan became eligible.
