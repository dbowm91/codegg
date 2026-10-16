# Agent Workstream Blackboard M006 — NewManaged Materializer Qualification Addendum

Status: closing — the filter-free NewManaged capability and daemon replay paths pass on the development Mac; native Windows qualification found Git for Windows' shorter repository-root limit and the next narrowed-path run is pending

Source implementation plan: `plans/implementation/agent-workstream-blackboard/006-session-worktree-launch.md`
Prior closure record: `plans/closure/agent-workstream-blackboard/006-status.md` (historical blocked finding retained)
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md`
Repository baseline: pending implementation commit for this qualification addendum

## 1. Executive finding

The prior NewManaged materializer blocker is resolved for filter-free pinned
commits. The service runs bounded tree and effective-attribute queries,
refuses active checkout filters before materialization, and disables hooks,
fsmonitor and recursive submodule checkout for the narrowly attested worktree
add. Existing Managed and NewManaged session launch/fork use the daemon-owned
reservation and workspace-binding saga. NewManaged launch/fork and exact
idempotent replay passed on the development Mac.

M006 is not strictly closed because the implementation plan requires native
Windows path-length and linked `.git`-file qualification. The focused
Windows workflow first exposed Git for Windows rejecting the verbatim
destination prefix, then exposed its `$GIT_DIR too big` failure with a roughly
225-character managed checkout root. Git for Windows has a shorter
repository-root limit than the general Windows `MAX_PATH` threshold. The
fixture now targets a roughly 200-character managed root on Windows while
keeping the 200–260 character fixture on other platforms. A native rerun is
required. This finding does not block M007, and neither milestone depends on
M005 shared-checkout mutation.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Bounded filter preflight distinguishes absent filter from explicitly configured drivers | `cargo test -p egggit filter_free_checkout_attestation_rejects_custom_filter_attributes --locked` | pass | Includes committed `filter=custom` and explicit `filter=unspecified` attributes. |
| Filter-free NewManaged checkout succeeds with hooks disabled | `worktree_service::tests::worktree_creation_materializes_filter_free_checkout_and_disables_hooks` | pass | Real Git worktree; post-checkout marker remains absent. |
| Filter refusal retains stable recoverable reservation and never runs smudge | `worktree_service::tests::stable_reservation_retries_same_checkout_and_retains_filter_refusal` | pass | Same worktree ID/path is retained; retry remains fail closed. |
| Concurrent and nested managed worktrees preserve distinct leases and pinned base | `concurrent_filter_free_worktree_creation_uses_distinct_leases`; `nested_worktree_creation_uses_the_pinned_filter_free_base` | pass | Real Git fixtures. |
| Daemon NewManaged launch/fork, session binding, ancestry and exact replay | `core::daemon::tests::managed_session_launch_and_fork_are_idempotent_and_bound` | pass | Local aarch64 macOS; includes existing-worktree and NewManaged launch/fork. |
| Linux hosted verification | Exact-head CI pending | not run | Record the existing CI run after pushing the implementation commit. |
| Deep checkout path and linked `.git`-file behavior | `managed_worktree_creation_materializes_filter_free_checkout` resolves Git metadata through the linked `.git` file; Windows uses a 190–209 character managed root within Git for Windows' repository-root limit, other platforms use 200–259 | local pass; three native runs have not passed | Run `38920436943` rejected the verbatim destination prefix; `38922769990` failed with `'$GIT_DIR' too big` while cwd retained the prefix; `38925119626` still failed with `'$GIT_DIR' too big` at an estimated 199-character fixture root after path-prefix normalization. The fixture is now shortened to about 199 characters on Windows; rerun before strict closure. |

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

## 5. Unresolved finding and disposition

| Severity | Finding | Required action |
|---|---|---|
| medium | Native Windows path-length and linked `.git`-file qualification still fails because Git for Windows rejects the prior roughly 225-character checkout root. | Rerun `.github/workflows/m006-windows-worktree.yml` with the Windows fixture shortened to about 199 characters and record its exact result; then decide strict closure from the complete M006 acceptance matrix. |

Keep M006 in `closing` until native Windows evidence is available. M007 remains independently eligible.
