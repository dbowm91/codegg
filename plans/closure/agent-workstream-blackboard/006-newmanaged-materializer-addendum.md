# Agent Workstream Blackboard M006 — NewManaged Materializer Qualification Addendum

Status: closed — NewManaged filter-free materialization, daemon launch/fork, exact-head hosted CI, and native Windows path/linked-metadata qualification passed

Source implementation plan: `plans/implementation/agent-workstream-blackboard/006-session-worktree-launch.md`
Prior closure record: `plans/closure/agent-workstream-blackboard/006-status.md` (historical blocked finding retained)
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md`
Qualified source SHA: `79cc725b223c8dbed399107f11679424c5eb4e7a` (M006 native Windows and CI qualification)

## 1. Executive finding

The prior NewManaged materializer blocker is resolved for filter-free pinned
commits. The service runs bounded tree and effective-attribute queries,
refuses active checkout filters before materialization, and disables hooks,
fsmonitor and recursive submodule checkout for the narrowly attested worktree
add. Existing Managed and NewManaged session launch/fork use the daemon-owned
reservation and workspace-binding saga. NewManaged launch/fork and exact
idempotent replay passed on the development Mac.

Native Windows qualification initially exposed destination/cwd path-prefix
handling and Git for Windows' shorter repository-root limit. The materializer
now normalizes both paths, and the Windows fixture targets a roughly
200-character managed root within that limit. The final native run and
exact-head CI passed; the test also resolves the linked `.git` file and checks
the shorter Linux temporary-root path. These M006 launch guarantees do not
depend on M005 shared-checkout mutation.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Bounded filter preflight distinguishes absent filter from explicitly configured drivers | `cargo test -p egggit filter_free_checkout_attestation_rejects_custom_filter_attributes --locked` | pass | Includes committed `filter=custom` and explicit `filter=unspecified` attributes. |
| Filter-free NewManaged checkout succeeds with hooks disabled | `worktree_service::tests::worktree_creation_materializes_filter_free_checkout_and_disables_hooks` | pass | Real Git worktree; post-checkout marker remains absent. |
| Filter refusal retains stable recoverable reservation and never runs smudge | `worktree_service::tests::stable_reservation_retries_same_checkout_and_retains_filter_refusal` | pass | Same worktree ID/path is retained; retry remains fail closed. |
| Concurrent and nested managed worktrees preserve distinct leases and pinned base | `concurrent_filter_free_worktree_creation_uses_distinct_leases`; `nested_worktree_creation_uses_the_pinned_filter_free_base` | pass | Real Git fixtures. |
| Daemon NewManaged launch/fork, session binding, ancestry and exact replay | `core::daemon::tests::managed_session_launch_and_fork_are_idempotent_and_bound` | pass | Local aarch64 macOS; includes existing-worktree and NewManaged launch/fork. |
| Hosted CI, including Linux workspace suite | CI `38930928833` on `79cc725b223c8dbed399107f11679424c5eb4e7a` | pass | 13,971/13,971 passed, 6 skipped; formatting, workspace Clippy, all listed guards, sandbox-helper build, and workspace tests passed. |
| Deep checkout path and linked `.git`-file behavior | `managed_worktree_creation_materializes_filter_free_checkout` resolves Git metadata through the linked `.git` file; Windows uses a 190–209 character managed root within Git for Windows' repository-root limit, other platforms use 180–259 | pass | Windows native workflow `38930920042` passed on the exact source SHA. CI `38930928833` also passed the portable Linux fixture with its 187-character temp-root path. Earlier failed runs and their successive corrections are preserved above. |

## 3. Implementation and invariant review

The generic Git policy continues to deny arbitrary `worktree add`. The only
new path accepts a private filter-free attestation for one canonical root and
full object ID, then constructs the fixed managed command. Custom filters fail
before checkout and the durable reservation remains visible for recovery.
Session requests retain their original source WorkspaceId for idempotent
replay; the created session binds to the newly registered target WorkspaceId.
Shared-checkout writes remain unavailable.

## 4. Verification executed

Pinned local toolchain: Rust 1.89 aarch64-apple-darwin. Commands used the
isolated target directory `/tmp/codegg-m006-qual-target` to avoid a concurrent
workspace target cleanup.

```bash
cargo test -p egggit filter_free_checkout_attestation_rejects_custom_filter_attributes --locked
RUSTC=.../1.89-aarch64-apple-darwin/bin/rustc RUSTDOC=.../1.89-aarch64-apple-darwin/bin/rustdoc \
  CARGO_TARGET_DIR=/tmp/codegg-m006-qual-target CARGO_INCREMENTAL=0 \
  rustup run 1.89-aarch64-apple-darwin cargo test -p codegg-core --lib \
    worktree_creation_materializes_filter_free_checkout_and_disables_hooks --locked
RUSTC=.../1.89-aarch64-apple-darwin/bin/rustc RUSTDOC=.../1.89-aarch64-apple-darwin/bin/rustdoc \
  CARGO_TARGET_DIR=/tmp/codegg-m006-qual-target CARGO_INCREMENTAL=0 \
  rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --lib \
    core::daemon::tests::managed_session_launch_and_fork_are_idempotent_and_bound --locked
RUSTC=.../1.89-aarch64-apple-darwin/bin/rustc RUSTDOC=.../1.89-aarch64-apple-darwin/bin/rustdoc \
  CARGO_TARGET_DIR=/tmp/codegg-m006-qual-target CARGO_INCREMENTAL=0 \
  rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --test work_orders_m002_materialization \
    managed_worktree_creation_materializes_filter_free_checkout --locked
RUSTC=.../1.89-aarch64-apple-darwin/bin/rustc RUSTDOC=.../1.89-aarch64-apple-darwin/bin/rustdoc \
  CARGO_TARGET_DIR=/tmp/codegg-m006-qual-target CARGO_INCREMENTAL=0 \
  rustup run 1.89-aarch64-apple-darwin cargo test -p codegg --test work_orders_m007_trajectory \
    f_worktree_containment_gate_and_truthfulness --locked
```

All eight focused tests passed. Formatting, `git diff --check`, the execution-
ownership guard, scheduler-bypass guard, core boundary guard, and forbidden
Git pattern guard passed. The first broad default-target test attempt was
stopped after unrelated integration binaries failed to link because an
x86_64 toolchain selected arm64 system libraries; the pinned arm64 unit and
trajectory commands above passed.

## 5. Residual limits and disposition

| Severity | Finding | Disposition |
|---|---|---|
| none | No unresolved M006 correctness finding remains within the accepted filter-free materializer scope. | Active custom checkout filters remain deliberately unsupported and fail closed with a retained reservation; adding a contained filter backend is a separate capability. |

## 6. Strict closure and downstream audit

M006 is strictly closed on implementation/source SHA
`79cc725b223c8dbed399107f11679424c5eb4e7a`. Hosted CI `38930928833` and the
native Windows worktree workflow `38930920042` both passed on that exact
source. The daemon-backed NewManaged launch/fork, session binding, replay,
reservation, filter refusal and recovery evidence listed above is complete.

Registry audit: M007's implementation and operational consumer gates were
also satisfied on this head, so M007 is strictly closed in
`plans/closure/agent-workstream-blackboard/007-daemon-rust-workspace-qualification.md`.
M008 is now active for isolated S0–S3 comparative qualification. Its S4
shared-checkout arm remains unavailable pending M005. M005 itself remains a
separate feasibility initiative; no writer-permit service or shared-write
authority is unblocked. No other registered plan became eligible.
