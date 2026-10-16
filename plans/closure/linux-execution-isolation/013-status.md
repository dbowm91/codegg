# Linux Execution Isolation C005 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/linux-execution-isolation/013-m003-bubblewrap-test-only-first-hop-and-feature-proof.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md#next-handoffs--explicit-test-only-vs-production-gates-2026-10-13`

Repository baseline reviewed: `5ff248d` (after C004 closure)

Implementation commits:

- `955b08df7c33e95ec7fb33468c6892f22b2f7f85` — opt-in Linux Bubblewrap first-hop fixture; production launch selection remains unchanged.
- `1c851aa9b35a138b627c757fb4eacfdfec6a3489` — assert typed production refusal and `no_new_privs` before the test payload.

## 1. Executive finding

The bounded C005 feasibility experiment is conditionally closed with positive native evidence for unprivileged Bubblewrap user, mount, PID, network, and IPC namespaces on this host. The fixture proves the child entered distinct namespaces, received `no_new_privs` before the shell payload, could not see the parent-owned status-pipe descriptor, and did not run after an invalid required mount.

This does not qualify the M003 protected mount backend. The test uses narrow runtime directory binds and a disposable writable fixture; it does not build protected nested Git/credential overlays, validate worktree aliases, or route production managed processes through Bubblewrap. No production backend was selected or registered. ADR-0035 was proposed when this closure was written and was accepted separately on 2026-10-14; this later decision does not promote a backend, qualify M003, or alter this test-only closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Real unprivileged namespace creation | Opt-in `tests/linux_bubblewrap_first_hop.rs` | pass | Target UID was 1000. Child user/mount/PID/net/IPC namespace inode IDs differ from the host. |
| Trusted fixed-path helper | Fixture metadata and version output | pass | `/usr/bin/bwrap`, UID 0, mode 0755, no setuid; Bubblewrap 0.9.0. SHA-256: `e318903862396f96de3df57264e0158682b952fd3fb53ac23d876413e7b30f71`. |
| Setup failure launches no target | Invalid required bind source in native fixture | pass | Bubblewrap exits unsuccessfully and the host-visible target marker remains absent. |
| Private status descriptor not inherited | Parent-owned test pipe, descriptor absence assertion in the child, parent-only receipt write | pass | This tests descriptor hygiene for the fixture; it is not the production helper status protocol. Existing `sandbox_landlock` tests separately reject child writes to the production private status endpoint. |
| Required no-new-privileges before payload | Fixture invokes trusted `/usr/bin/setpriv --no-new-privs` before the shell payload and reads the child status | pass | Bubblewrap itself did not provide this property in the observed invocation; the test-only first-hop wrapper supplied it. Production integration must own and prove this ordering. |
| Existing launch contract refuses unqualified production routing | `ProductionLinuxLaunchAdapter` assertion in fixture | pass | Required network/process dimensions remain `EnhancedUnavailable`; the test-only path is not production registered. |
| Helper/status malformed, duplicate, or oversized frame rejection | `status_decoder_rejects_malformed_duplicate_and_oversized_frames` | pass | 1/1 focused unit test. |
| Landlock, ordinary containment, and process cleanup regressions | Focused test commands below | pass | 7 Linux launch, 20 managed-process, 14 containment, 3 Landlock, and 5 descendant tests passed. |
| Setup overhead | 25-run host `true` versus Bubblewrap namespace `true` microbenchmark | partial | Host median 0.753 ms/p95 0.876 ms; Bubblewrap median 5.909 ms/p95 6.163 ms. This is process-start overhead only, not Cargo/toolchain or PTY compatibility. |
| Full toolchain and PTY compatibility under Bubblewrap | Not run | not run | Cargo metadata, rustc, Python, Git workspace operations, and PTY behavior remain unqualified for this fixture. |

Host: Ubuntu 24.04.5 LTS, x86_64, kernel `6.8.0-146-generic`; `user.max_user_namespaces=28633`; AppArmor enabled and current profile `unconfined`; SELinux absent; `/proc/self/status` reports host `Seccomp: 0`. The actual child namespace test is authoritative here: util-linux `unshare --user --map-root-user --mount true` failed while writing `/proc/self/uid_map`, but Bubblewrap's direct unprivileged namespace path succeeded. Cgroup delegation was not part of C005 and remains unproven.

Observed namespace IDs:

| Namespace | Host inode | Child inode |
|---|---:|---:|
| user | 4026531837 | 4026532585 |
| mount | 4026531841 | 4026532517 |
| PID | 4026531836 | 4026532519 |
| network | 4026531840 | 4026532520 |
| IPC | 4026531839 | 4026532518 |

## 3. Production implementation evidence

The change adds only an opt-in integration fixture. It resolves Bubblewrap from fixed system paths, validates root ownership and non-writable ancestry, does not bind the host root, creates private namespaces, bind-mounts the temporary fixture directory, applies `no_new_privs` before the shell payload, and bounds the helper wait to ten seconds. It does not alter `ManagedProcessService`, production backend selection, sandbox defaults, or public configuration.

## 4. Verification executed

### Commands run

```bash
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target CODEGG_RUN_LINUX_BWRAP_FIRST_HOP=1 cargo test --test linux_bubblewrap_first_hop --locked -- --nocapture
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test -p codegg --lib security::sandbox::linux_launch --locked -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test -p codegg --lib managed_process --locked -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test -p codegg --lib status_decoder_rejects_malformed_duplicate_and_oversized_frames --locked -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test sandbox_containment --locked -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test sandbox_landlock --locked -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test managed_process_descendants --locked -- --test-threads=1
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
scripts/verify.sh quick
```

### Results

All listed commands passed locally. Test counts were respectively 1, 7, 20, 1, 14, 3, and 5. `scripts/verify.sh quick` passed on the final fixture revision, including formatting, repository guards, and `cargo check --workspace --all-targets --locked`. Hosted CI was not run.

## 5. Invariant review

- The production adapter continues to return `EnhancedUnavailable` for required enhanced dimensions.
- No command syntax, permission, profile, network, or FullHost authority changed.
- Failed required mount setup leaves no target marker.
- Bubblewrap helper identity comes from fixed paths and root-owned, non-writable ancestry, not PATH or workspace content.
- The native test does not claim protected nested mounts, seccomp, inherited-FD sanitation beyond the status descriptor, host IPC denial, offline routing, or resource limits.

## 6. Failure and recovery review

The test uses disposable temporary storage and synthetic marker files. Helper wait is bounded to ten seconds; the test kills and reaps a timed-out helper. Managed-process timeout/cancel and descendant tests passed separately. Concurrent Bubblewrap attempts and namespace PID-1 descendant cleanup were not exercised in this first-hop fixture.

## 7. Migration and compatibility review

No storage, protocol, or config migration occurred. Existing Landlock-only behavior and unrestricted network status remain unchanged. Cargo/toolchain, Python, Git workspace operations, and PTY compatibility under the test namespace remain open gates for M003 design.

## 8. Security review

The fixture verifies real namespace identity and no-target-on-setup-failure. It binds only `/usr/bin`, `/usr/lib`, `/lib`, `/lib64`, `/proc`, `/dev`, and disposable scratch; it does not bind `/` or expose `/run`, host credentials, daemon sockets, or host workspace. The test-only `setpriv` wrapper makes `no_new_privs` observable before payload execution. No production trust claim derives from this fixture.

## 9. Documentation and operations

The fixture is opt-in with `CODEGG_RUN_LINUX_BWRAP_FIRST_HOP=1`. Its exact invocation, host metadata, helper version, and namespace inode observations are recorded above. Existing ADR-0035 and production rollout gates remain in the Linux roadmap.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Bubblewrap first-hop is not integrated with the canonical managed-process owner or production private receipt protocol. | No production route may select this backend. | C006 must integrate only after a separate ADR-0035 decision and its full real protected-mount requirements. |
| medium | Toolchain, Git workspace mutation, PTY, namespace PID cleanup, and concurrent fixture compatibility were not qualified. | M003 production compatibility and lifecycle remain unproven. | Qualify these in a later accepted, bounded implementation plan; do not infer them from this test. |
| medium | No full Landlock ABI number was emitted by this fixture. | Existing Landlock tests establish compatibility but this closure does not claim an ABI value. | Capture exact ABI in a later host qualification record if required. |

## 11. Roadmap disposition

C005 is conditionally closed as a bounded positive namespace feasibility experiment. It does not close M003 or qualify production Bubblewrap. C006 remains blocked on separate explicit ADR-0035 disposition, C005 evidence appropriate to its production scope, and M002. C007 and C008 remain eligible for their fixtures-only feasibility scopes.

## 12. Registry updates

- Move C005 from dependency-ready work to recently closed with implementation commit `955b08df7c33e95ec7fb33468c6892f22b2f7f85` and this closure link.
- Mark C005 conditionally closed in the Linux roadmap, with toolchain/PTY compatibility listed as outstanding evidence.
- Unblock audit: no plan became ready due to C005. C006 remains blocked on its other hard gates; C007/C008 were already ready.
