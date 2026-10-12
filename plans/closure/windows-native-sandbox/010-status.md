# Windows Native Sandbox Corrective C010 — Closure Status

Status: closed (test-only infrastructure)

Source implementation plan:

- `plans/implementation/windows-native-sandbox/010-windows-acl-leases-and-recovery.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`

Repository baseline reviewed: `83c29a1298c54a9b495b7656b8478cf2eb340ea0`

Implementation commits or pull requests:

- `7b4f2e86` — verify ACL lease read access and cancellation restoration
- `05275b7b` — remove PowerShell module dependencies from AppContainer fixture
- `75a601b5` — report unavailable ACL lease roots as typed sandbox failures
- `097a3f5b` — persist test-only ACL lease journals under the per-user daemon root
- `1f3245ba` — record root owner SID and reject roots owned by unrelated principals
- `83c29a12` — distinguish and accept only the elevated token's user or owner SID

## 1. Executive finding

C010 closes as a bounded test-only ACL lease and recovery infrastructure milestone.
Run `38211656312` exposed that an elevated GitHub runner assigns disposable roots to
the token's default owner SID (the Administrators group), not the `runneradmin` user
SID. Commit `83c29a12` now accepts a root only when its owner SID matches either the
current token user or its configured default owner. Same-revision Windows Server
2025 workflow `38215171043` passed the owner-bound ACL integrations and recovery
suite. The closure remains limited to runner-owned disposable fixtures.

The fixture remains explicitly test-only. It stores its journal under the per-user
daemon root, and production Windows constrained-backend admission remains disabled.
The hosted fixture does not qualify arbitrary existing workspaces, protected
credentials, daemon IPC, or adversarial reparse/hardlink/ADS cases. Those are C011
gates; this closure does not change the historical blocked M004 record or claim
`filesystem=Enforced`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Journal a run, AppContainer SID, path owner, path identity, original DACL, and expected leased DACL before mutation | `src/managed_process/windows_acl_lease.rs`; six lease unit tests in run `38209481698` | pass | Versioned record uses atomic create and durable write; path lock serializes a root. |
| Restore only when path identity and descriptor state match; retain conflicts | Unit tests for descriptor drift and path identity reuse in run `38209481698` | pass | Conflicting records are preserved and recovery refuses to overwrite drift. |
| Roll back a partial mutation and recover a simulated interrupted lease | Unit tests for injected ACL-write failure and simulated crash recovery in run `38209481698` | pass | Recovery API is exercised on the hosted Windows filesystem. |
| Prevent same-root concurrent leases from removing each other's ACE | Same-path contention unit test in run `38209481698` | pass | Lock contention is bounded. |
| Run a restricted child against a leased disposable root; allow approved access and deny outside access | Two `windows_acl_lease` integration tests and six `windows_restricted_managed_process` tests in run `38209481698` | pass | Includes allowed write/read, read-only read and denied write, synthetic outside read/write denial, and cancellation cleanup. |
| Restore exact DACL state and remove completed journal records | ACL integration tests in run `38209481698` | pass | The fixture asserts before/after descriptor equality and no remaining JSON lease records. |
| Reject unavailable/missing lease roots before child creation | `missing_filesystem_root_fails_before_the_test_child_is_created`; run `38209481698` | pass | Returned as typed `SandboxFailed`. |
| Accept roots owned by the elevated runner's user or configured token owner while rejecting unrelated owners | `current_user_sid`/`current_token_owner_sid`; run `38215171043` | pass | The owner comparison is bounded to the active token's two documented SID identities. |
| Exercise actual daemon restart/startup recovery and protect the journal from another local principal | No production startup caller or cross-principal test exists | partial | The recovery engine is tested directly; production Windows backend remains disabled. C011 must integrate and qualify the privileged boundary before admission. |
| Qualify existing user workspaces, credentials, daemon IPC, reparse points, hardlinks, ADS, and UNC paths | Not run | not run | Explicit C011/M004 adversarial scope; no hosted disposable-root result is inferred for these cases. |

## 3. Production implementation evidence

The Windows ACL lease module is compiled only for Windows test-support builds and
exposes a bounded crate-internal lease/recovery engine. A journal record binds the lease to a run,
AppContainer SID, canonical directory, volume serial, file index, original DACL,
and expected leased DACL. Mutation writes the complete record first, applies only
the scoped AppContainer ACE, and restores only after checking identity and expected
descriptor state. Per-path file locks serialize acquisition and recovery; drift,
unknown journal versions, malformed records, and identity reuse are retained as
conflicts rather than repaired destructively.

The test-only restricted launch path passes the journal directory under
`LocalDaemonPaths::resolve().root/sandbox/acl-leases` and calls `ensure_root()`.
The production Windows backend still has no caller and constrained requests remain
unavailable. No user-visible filesystem capability is claimed by this infrastructure
closure.

## 4. Verification executed

### Commands run

```bash
# GitHub Actions run 38209481698, revision 097a3f5b3f6c98c157c308fd1f97f7e0e24b9a86
cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --lib managed_process::windows::tests::failed_job_assignment_never_runs_the_suspended_target -- --nocapture
cargo test --locked -p codegg --features windows-sandbox-test-support --lib managed_process::windows::tests::restricted_ -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --lib managed_environment_block_is_accepted_by_create_process -- --nocapture
cargo test --locked -p codegg --lib status_decoder_rejects_malformed_duplicate_and_oversized_frames -- --nocapture
cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_restricted_managed_process -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_acl_lease -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --lib windows_acl_lease -- --nocapture --test-threads=1
cargo test --locked -p codegg --lib managed_process::windows::tests::appcontainer_ -- --nocapture
# plus seven selected Windows shell/policy commands in the same workflow
bash scripts/verify.sh quick
cargo test --locked -p codegg --features windows-sandbox-test-support --target x86_64-pc-windows-gnu --test windows_acl_lease --no-run
cargo test --locked -p codegg --features windows-sandbox-test-support --target x86_64-pc-windows-gnu --test windows_restricted_managed_process --no-run
cargo fmt --check --all
git diff --check
```

### Earlier results

- GitHub Actions run [`38209481698`](https://github.com/dbowm91/codegg/actions/runs/38209481698) passed at the exact reviewed revision. Host: Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`; Rust `1.89.0 (29483883e 2025-08-04)`, target `x86_64-pc-windows-msvc`.
- Windows counts: launcher 8; suspended-target failure injection 1; restricted stdio setup failures 3; managed environment 1; status-frame decoder 1; restricted service integration 6; ACL lease integration 2; ACL lease/recovery unit tests 6; AppContainer ordering/cleanup 5; and the selected policy/shell tests all passed.
- `bash scripts/verify.sh quick`, Windows GNU cross-target test compilation, formatting, and whitespace checks passed on the reviewed revision.
- Prior hosted attempts recorded during this work exposed and corrected descendant PID observation (`38200796202`), Windows path/assertion compile errors (`38202634103`), stale C009 status assertions (`38203699252`), and unavailable PowerShell module commands in AppContainer (`38205339461`). Run `38207255455` passed before the final daemon-root adjustment; same-revision run `38209481698` passed after it. Redundant run `38209010743` was cancelled.
- Run `38214196715` passed the elevated-runner probe and baseline launcher tests but failed compiling the first feature-gated test: a concurrent upstream token-owner change combined with the local correction and duplicated `current_token_owner_sid` (`E0428`). The helper was deduplicated, `TokenUser` handling retained, and Windows GNU cross-target compilation passed before the next hosted run.
- Privilege probe in run `38211656312`: identity `runnervmfi6oq\\runneradmin`; Administrator role `True`; high-integrity token `True`; UAC enabled `True`. The elevated token satisfies the disposable-runner privilege gate. That run later failed both ACL integration tests because the initial owner check recognized only the user SID.

### Final owner-bound verification

- GitHub Actions run [`38215171043`](https://github.com/dbowm91/codegg/actions/runs/38215171043) passed on revision `83c29a1298c54a9b495b7656b8478cf2eb340ea0`, Windows Server 2025 Datacenter build `26100`, Rust `1.89.0`, target `x86_64-pc-windows-msvc`.
- The run passed launcher tests, suspended-target and restricted-stdio failure injection, managed environment and status-frame tests, six restricted managed-process integration tests, two ACL lease integration tests, six ACL/recovery unit tests, five AppContainer ordering/cleanup tests, and selected native shell/policy tests.
- Its privilege probe reported Administrator role `True`, high-integrity token `True`, and UAC enabled `True`. ACL integration assertions verified the elevated runner-owned roots and exact descriptor restoration.
- Run `38214196715` failed during Windows test-support compilation because the owner-SID correction duplicated `current_token_owner_sid` and referenced a missing `current_user_sid`. Commit `83c29a12` separates the TokenUser and TokenOwner queries; the final run above compiled and passed those paths.
- The local Windows cross-target `cargo check` could not locate the target's `core` crate in the active toolchain. This does not replace or qualify the successful hosted Windows execution; earlier GNU cross-target and `verify.sh quick` evidence remains tied to revision `097a3f5b`.

## 5. Invariant review

- No global or broad `Everyone`/`Users` ACL mutation is performed by the lease engine.
- Every leased path is identified by canonical path, volume serial, and file index; expected descriptor drift blocks restoration.
- The durable before-image is written before DACL mutation. Partial mutation failure attempts restoration and preserves the record if restoration cannot be established.
- A child is not launched if a requested test fixture root is missing or unavailable.
- The production Windows constrained backend stays unavailable, and the fixture's result cannot be mistaken for `Enforced`.
- Network remains unrestricted; this milestone makes no network-containment claim.

## 6. Failure and recovery review

Injected ACL-write failure exercises rollback. Simulated crash recovery exercises
the on-disk record after the lease owner's lock is released. Identity reuse and DACL
drift retain the journal and refuse an unsafe restore. Contending leases serialize
on a per-path lock, and lock acquisition has a ten-second bound. Normal release,
drop cleanup, child cancellation, and Job teardown are covered by hosted integration
tests.

The test suite does not kill the daemon process during each individual ACL write
transition, nor does this closure claim recovery is wired into daemon startup.
Those operational integration details must be resolved before a production backend
can consume the lease engine.

## 7. Migration and compatibility review

No database schema or public protocol changed. Journal records are versioned; an
unknown version is retained as a conflict. The lease root follows the per-user
daemon root and `CODEGG_DAEMON_HOME` override via `LocalDaemonPaths`. Unix behavior
is unchanged. No existing workspace ACL is adopted or migrated.

## 8. Security review

The durable record includes the DACL before-image and therefore must remain within
the daemon-owned storage boundary. This milestone verifies its selected location
and exact behavior on the hosted runner but does not establish protection against
another local principal or a compromised same-user process. Existing host paths,
credential files, daemon named pipes, reparse races, hardlinks, ADS, UNC paths, and
unsupported filesystems remain unqualified. The production backend is disabled
while those C011 requirements remain open.

## 9. Documentation and operations

- `docs/windows-acl-leases.md` documents journal location, root failure behavior,
  recovery/conflict semantics, and test-only status.
- `architecture/process-tool-execution-ownership.md` records lease storage and the
  lack of a production Windows backend caller.
- `architecture/security.md`, `architecture/testing.md`, the Windows roadmap, and
  registry preserve the boundary between disposable-root infrastructure and M004
  security qualification.
- No operator repair command is provided; conflicts are retained for evidence and
  production admission remains disabled.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | The lease engine is not connected to production backend admission or daemon startup recovery, and a per-user journal has not been adversarially protected from another local principal. | Windows filesystem containment cannot be offered as an enforced production capability. | C011 must establish the privileged boundary, integrate/reconcile startup recovery, and keep admission unavailable for unsupported hosts. |
| high | Existing workspace, real credentials, daemon IPC, reparse/junction, hardlink, ADS, and UNC behavior is unqualified. | M004 remains blocked; no M005/M006 unblocking. | C011 adversarial matrix and additive M004 requalification record. |
| low | Windows client builds and non-NTFS/remote-volume behavior have not been qualified. | Support matrix remains narrow to the tested hosted server fixture. | C011 records supported host/filesystem boundaries or explicit unavailability. |

These findings are outside C010's bounded disposable-root infrastructure acceptance;
they remain explicit blockers to C011/M004 and are not reported as resolved.

## 11. Roadmap disposition

C010 is closed for the disposable-root infrastructure acceptance above. The hosted
runner meets C011's operational privilege gate (Administrator role and high token,
with UAC enabled). C011 is now eligible to proceed. M004 remains blocked; C010
does not establish filesystem enforcement or backend readiness.

## 12. Registry updates

- Mark C010 closed for test-only infrastructure and move its implementation plan
  to implemented.
- Move C011 to active: C010 is closed and the elevated hosted-runner gate passed.
  Keep unrun adversarial cases listed as C011 work, not as passed evidence.
- Do not alter `plans/closure/windows-native-sandbox/004-status.md`.
