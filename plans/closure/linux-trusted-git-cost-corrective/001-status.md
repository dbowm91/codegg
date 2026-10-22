# Linux Trusted Git Cost Corrective C001 — Closure Status

Status: conditionally closed — test-only host-authority boundary established; production live-ODB use remains prohibited

Source implementation plan:

- `plans/archive/implementation/linux-trusted-git-cost-corrective/001-live-odb-trust-model-and-negative-probes.md`

Source subsystem roadmap:

- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md#milestones-and-handoffs`

Repository baseline reviewed: rebased branch baseline `0b36c71f`; exact hosted test head `326a9f042f9d98c12175be5cee5a7cb5d595b787`.

Implementation commits or pull requests:

- `5c43a7fdfbe667f51387cbd4f8a429a510a0b599` — add test-only contained live-ODB Git alias and external-diff probe.
- `eb15d21726bbff86a90988a89828f0195ac12d35` — add the threat matrix and C003 benchmark evidence.
- `326a9f042f9d98c12175be5cee5a7cb5d595b787` — reconcile the TUI test adapter with the rebased `InteractiveProcessProtocol::create` signature and close C003.
- Exact-head hosted Linux qualification: [run 39102154352](https://github.com/dbowm91/codegg/actions/runs/39102154352), success.

## 1. Executive finding

C001 establishes a test-only host-authority boundary for a live Git object read on the qualified hosted Linux configuration. A repository-local alias and `diff.external` command ran inside `ManagedProcessService` with actual M004 process/IPC and M005 offline-network guarantees. The commands could not read a host-only credential sentinel or host proc entry, connect to a host loopback listener, or share the host network namespace. A separate no-userns runner proved required M004 launches, constrained PTY, and constrained LSP refuse before target execution.

This does not make the live object view immutable or generation-consistent. It also does not qualify external gitdirs/common-dirs, linked/nested repositories, alternate object stores, replace/graft/promisor state, concurrent object writers, or every Git executable configuration surface. Keep the live-ODB route test-only. C002 may proceed only as an internal test-only pilot for the exact covered root shape; ADR-0044 remains proposed and production promotion remains blocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Separate host-authority from consistency claims | `research/linux-trusted-git-cost-corrective/c001-live-odb-threat-matrix.md`; C012's prior no-writer-lease rejection | pass | Read-only mount/M004/M005 do not establish a stable multi-object generation. |
| Use pinned Git and required daemon-owned process boundary | `tests/linux_m004_process_ipc.rs::test_only_contained_git_reads_live_objects_and_contains_configured_command`; exact hosted run 39102154352 | pass | Canonical Git executable path; no production Git route or selector added. |
| Prove a live-object read and configured command execution | Contained `git log`, `git diff` with `diff.external`, and repository-local alias probe | pass | Configured command marker was written in the workspace. |
| Deny host credentials, proc, loopback, and network | Credential/proc/network markers plus host-positive loopback control and child network namespace inode; exact hosted run 39102154352 | pass | No leak/network marker; child namespace inode differed from host. Existing M005 TCP/UDP/inherited-socket tests also passed in the same job. |
| Fail closed on a host without user namespaces | Restricted-userns job in run 39102154352: required M004, constrained PTY, and constrained LSP refusal/no-target checks | pass | The ephemeral runner set `user.max_user_namespaces=0`. |
| Cover all Git-native executable surfaces and path expansion | Threat matrix rows for hooks, fsmonitor, filters, merge drivers, textconv, credential/SSH helpers, linked/external gitdirs and object metadata | partial | Generic configured alias and external diff prove arbitrary configured child execution stays within the tested boundary; individual surfaces and path expansion remain untested. |
| Exercise concurrent host writers and live-generation races | C012 evidence rejects the in-process no-writer lease; C001 threat matrix records race semantics | partial | No deterministic concurrent `gc`/repack/object replacement fixture was added. No immutable-generation claim is made. |
| Keep production backend unchanged | `crates/egggit/src/process.rs` still uses the private immutable object view; no production selector added | pass | Historical C010 no-admission and C012 negative decisions remain active. |

## 3. Production implementation evidence

No production route changed. The native fixture is opt-in and calls the existing `ManagedProcessService` with a `SandboxRequest::Required` configuration that requests M004 and M005. It reads a normal repository's original objects in place and runs configured Git commands inside the same child boundary. Production `GitEnvPolicy` retains its private immutable object-view behavior. No public result field, DTO, database, or backend-selection setting was added.

## 4. Verification executed

### Commands run

```bash
# Local compile and repository verification
cargo test --locked -p codegg --lib lsp::m004::tests::constrained_lsp_launch_fails_before_target_when_userns_are_restricted --features lsp-test-support -- --test-threads=1 --nocapture
cargo test --locked -p egggit trusted_git -- --test-threads=1
cargo test --locked -p egggit -- --test-threads=1
scripts/verify.sh quick
python3 scripts/check_adr_integrity.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_sandbox_contract.py

# Hosted exact-head Linux qualification, run 39102154352
cargo test --locked --test linux_m004_process_ipc -- --nocapture
cargo test --locked -p codegg --lib interactive_process::tests::m004_live_owner_preserves_tty_and_private_process_view -- --test-threads=1 --nocapture
cargo test --locked --test interactive_process_attach_resume m004_daemon_constrained_pty_uses_principal_profile_and_cancels_descendant -- --nocapture
cargo test --locked -p codegg --lib lsp::m004::tests::constrained_lsp_process_is_owned_by_m004_scope --features lsp-test-support -- --test-threads=1 --nocapture
cargo test --locked --lib protected_mount_ -- --nocapture
cargo test -p egggit --locked trusted_git -- --test-threads=1
```

### Results

- Exact-head hosted process/IPC suite: 10 passed, including the contained live-ODB Git probe; total suite time 501.74 seconds.
- Hosted live PTY owner: 1 passed; daemon-authorized PTY route and descendant cleanup: 1 passed; constrained LSP duplex/procfs: 1 passed.
- Hosted protected-mount regressions: 11 passed; egggit trusted-Git regressions: 20 passed.
- Hosted restricted-userns checks: required M004 refusal, constrained PTY refusal, and constrained LSP refusal each passed. No target marker was produced.
- Hosted runner: Ubuntu 24.04 x86_64, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0-1ubuntu0.3`, `/usr/bin/bwrap` mode `0755`, 63,795 max user namespaces for positive qualification, seccomp action support present, AppArmor profile `unconfined`. The workflow relaxed `kernel.apparmor_restrict_unprivileged_userns` only in the ephemeral positive VM. The restricted job set `user.max_user_namespaces=0`.
- Local trusted-Git tests: 20 passed. Full egggit serial suite: 108 passed, 1 ignored. An un-serialized full egggit run had two failures due to process-global test interaction; both passed under the serial execution required by repository guidance.
- `scripts/verify.sh quick`, ADR integrity, execution ownership, Git forbidden-pattern, sandbox contract, formatting, and diff checks passed. Hosted x86_64 macOS target compile passed in run [39102146034](https://github.com/dbowm91/codegg/actions/runs/39102146034).
- Local positive namespace proof was unavailable: this host's `unshare -Ur true` returns `Operation not permitted`. The hosted exact-head proof replaces only that host-specific positive evidence.

## 5. Invariant review

- Read-only access to live objects is not treated as immutable or generation-consistent.
- M004/M005 were requested and reported as actually enforced on the child before the probe ran; target output confirmed the separate network namespace.
- A configured Git command ran only as a child of the contained Git process and did not acquire host home/proc/network access in the tested host configuration.
- Unsupported namespace setup remains fail-closed; hosted restricted-userns checks demonstrate no-target behavior.
- No FullHost retry, implicit credentials, production selector, or accepted ADR-0044 authority was introduced.
- External host object mutation and metadata expansion remain explicit no-admission conditions.

## 6. Failure and recovery review

The initial exact-head hosted run 39100634870 did not complete: it exposed a rebased test-adapter call missing the new `SandboxProfile` argument. The test seam was corrected to pass `FullHost`, matching its existing TUI test contract. The corrected exact-head run 39102154352 passed the complete Linux workflow. The hosted restricted-userns tests confirmed refusal before target; the positive live test runs under the M004/M005 owner and has bounded process timeout. The probe creates only temporary files in its temporary workspace and HOME sentinel directory.

The test does not inject external `gc`/repack races, cancellation during a multi-object read, or process crash while an ODB generation changes. CodeGG must continue to treat those results as potentially inconsistent until a separate generation contract exists.

## 7. Migration and compatibility review

No schema, protocol, user setting, root lockfile, or production dependency changed. Windows and macOS Git execution routes remain untouched. The hosted macOS compile passed on the same exact branch head. There is no rollout or rollback because the test fixture is opt-in and no runtime selection changed.

## 8. Security review

The live Git executable was resolved to a canonical host path and launched through `ManagedProcessService`; `SandboxRequest::Required` prevented a fallback. The hosted probe had an independent host loopback positive control, used host-only credential/proc markers, recorded the child network namespace, and confirmed no access markers appeared. The test also required the configured alias and external diff command to actually run, avoiding a false pass caused by Git suppressing them.

This only qualifies the Ubuntu 24.04 runner configuration documented above. It does not establish behavior on AppArmor/SELinux variations, older kernels, external Git metadata roots, or every Git version. It provides no production authorization or data-consistency guarantee.

## 9. Documentation and operations

- Added and updated `research/linux-trusted-git-cost-corrective/c001-live-odb-threat-matrix.md` with exact-head proof, runner facts, and open vectors.
- Exact-head native positive and restricted-host coverage is in `.github/workflows/linux-m004-native.yml`.
- Passed the ADR, process ownership, Git forbidden-pattern, sandbox contract, and quick verification guards.
- Operator behavior remains fail-closed on unsupported namespace/userns configurations.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Live ODB has no immutable multi-object generation contract and can race external Git writers | A successful contained command can still observe a mixed or changing object generation | Keep production reads on immutable snapshots; do not promote without an accepted ADR and external-writer/generation design. |
| medium | Hooks, fsmonitor, clean/smudge/process filters, merge drivers, textconv, credential helpers and SSH commands were not individually invoked | Version-specific behavior is not independently characterized | C002/C005 must test applicable executable paths under the same obtained authority and keep unsupported actions typed as unavailable. |
| medium | External gitdir/common-dir, linked/nested repositories, alternates, replace refs, grafts and promisor metadata were not tested | A future adapter could expose paths or alter object resolution outside this fixture | Reject unknown roots/alternates or prove bounded reach before broadening C002's fixture. |
| medium | No deterministic concurrent repack/loose-write race test | The outcome under source mutation is not characterized | Keep no-consistency language; add explicit detection or typed stale/unavailable behavior before production use. |
| high | ADR-0044 is still proposed and M005 remains candidate-only for production admission | No new read backend can be enabled by this closure | Require separate ADR acceptance, M005 production-owner qualification, and independent C010/M003 admission. |

## 11. Roadmap disposition

C001 is conditionally closed for its test-only host-authority question. C002 may proceed with an internal test-only contained-read pilot limited to the ordinary repository root proven here; all production/live-ODB promotion stays blocked by the unresolved findings above. C004 test-only transaction modeling may proceed against the explicit consistency and authority limitations. C005 remains blocked on C002/C004, M005 production admission, ADR-0044, and independent M003/C010 review. C010's no-admission and C012's rejection of the no-writer lease remain unchanged.

## 12. Registry updates

- Mark C001 conditionally closed and link this closure and the archived plan.
- Make C002 test-only eligible while retaining its production blockers.
- Keep C004 test-only design eligible; no broker publication or broad `.git` writes are admitted.
- Keep C005 blocked and ADR-0044 proposed.
