# Windows Native Sandbox Corrective C012 — Existing-Tree Deny Lease Correction

Status: active

Repository baseline: `6efc0f92ab50fbadd10f3e1188e3639a4202afcd`, branch `work/windows-native-sandbox-foundation`, 2026-10-12.

Corrects C011's hosted adversarial finding: `plans/closure/windows-native-sandbox/011-status.md`.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`.

Long-term requirements:
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`. No new architectural decision is authorized.

Primary class: invariant

## 1. Objective

Diagnose and correct the test-only Windows ACL lease behavior that let an AppContainer read a synthetic credential file beneath a denied existing directory, including through a junction and hardlink. Preserve exact rollback and crash-safe journal semantics. If a safe bounded correction cannot be demonstrated, reject the affected policy before child creation and keep Windows constrained execution unavailable.

This is a corrective of C011's failed security qualification, not a production-backend enablement plan. M004 remains blocked until all of C011's requirements pass and a separate additive requalification is accepted.

## 2. Why this milestone is ready / dependency gate

Hard: C011 produced same-revision Windows Server 2025 evidence in run `38217933472`: direct, junction, and hardlink reads returned the synthetic credential contents despite the credential directory being declared denied; the alternate data stream read was denied. C010's owner-bound test-only lease/recovery infrastructure is available. The run `38217662482` at implementation revision `657587b9` passed the newly added final-component reparse-root rejection test and the rest of its Windows workflow.

## 3. Current implementation evidence

- `run_windows_test_restricted` leases DACLs on the root directories in `read_paths`, `write_paths`, and `deny_paths`.
- `windows_acl_lease::add_appcontainer_ace` adds inheritable AppContainer ACEs to a directory DACL. Existing files can also have their own effective DACLs and explicit ACE ordering; root-only success is not proof that existing file objects deny access.
- The C011 test used only runner-created directories and synthetic credential contents. It did not read repository, runner, or user secrets.
- Microsoft documents that file and directory access is decided by their ACLs and that explicit ACEs take precedence over inherited ACEs: [file security and access rights](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights), [automatic propagation of inheritable ACEs](https://learn.microsoft.com/en-us/windows/win32/secauthz/automatic-propagation-of-inheritable-aces). The exact observed fixture DACL ordering and access path must be captured as part of this correction rather than inferred from the directory-only snapshot.

## 4. Invariants that must not regress

- A denied existing file remains unreadable through its original path, any junction/reparse route, hardlink, and alternate data stream.
- No child resumes until every requested restriction has been independently verified or the request fails with a typed unavailable result.
- Journaling precedes every DACL mutation; recovery restores only objects whose identity and expected descriptor still match.
- No broad user, group, or system ACE is removed or weakened. Third-party ACL drift is preserved as a conflict.
- The entire lease engine stays test-support-only. Production backend admission remains disabled.

## 5. Scope

### In scope

- Reproduce and record the exact root and descendant DACLs and access-check route from C011.
- Select a bounded mechanism that safely denies existing descendant objects or rejects unsupported trees before child creation.
- Extend the journal to cover every object changed by a recursive/tree lease, if needed, including path/volume/file identity, owner, before-image, expected after-image, and conflict state.
- Bound traversal and fail closed on reparse points, unsupported filesystems, inaccessible objects, identity aliases that cannot be safely tracked, and object-count/descriptor-size limits.
- Re-run C011's synthetic direct/junction/hardlink/ADS matrix and rollback/crash fault-injection cases on the hosted privileged runner.

### Explicitly out of scope

- Production `SandboxRequest::Required` backend, user workspace adoption, real credentials, daemon IPC, named-pipe qualification, UNC support, network controls, and M004 closure.
- Broad ACL normalization, taking ownership, changing unrelated ACEs, or weakening user-visible availability failures.

## 6. Required implementation changes

- Compare the effective DACL on the denied root, its existing directories/files, and the synthetic credential file before and during the lease. Record explicit/inherited ACE flags and canonical order without exposing credential contents.
- Prove whether inheritable deny ACE propagation reaches the current descendant objects and whether an existing explicit allow or protected DACL bypasses the intended deny. Do not rely on a root-only ACL snapshot.
- Implement the smallest safe lease transaction. Every changed object must have a durable before/after record written before mutation. Partial mutation rolls back in reverse order; restart recovery validates every object identity and descriptor before restoring any object. If atomic all-tree guarantees cannot be met, reject that denied root before creating the child.
- Reject reparse points anywhere in a traversed deny tree until a safe, identity-pinned policy exists. Detect hardlink identity aliases where relevant; do not mistake path uniqueness for file-object uniqueness.
- Add a negative test that asserts the child never starts when the denied tree cannot be safely leased.

## 7. Ordered work packages

### WP-A — Reproduce and diagnose

Capture DACLs and file identities for root, existing secret file, hardlink, junction target, and ADS owner file. Identify the ACE ordering/access-check reason for the leak. Keep all contents synthetic.

### WP-B — Bounded lease correction

Implement either journaled existing-tree DACL protection with conflict-safe recovery, or a deterministic pre-spawn rejection for trees that cannot meet the invariant. Preserve rollback under injected failures at each mutation boundary.

### WP-C — Native adversarial verification

Run C011's exact synthetic path matrix on Windows Server 2025. All denied reads must fail while approved workspace reads/writes continue. Assert exact descriptor restoration for every touched object after success, failure, cancellation, and simulated restart recovery.

## 8. Failure, cancellation, restart, and contention semantics

Bound the transaction and traversal. A failed preflight or partial mutation must prevent child creation. Recovery restores only when all changed objects match their journaled identity and expected leased descriptor; otherwise retain the records and report conflict. Concurrent leases for overlapping trees must serialize or fail before mutation. Never release the lease before the Job and descendants have terminated.

## 9. Compatibility and migration

This remains a Windows-only, test-support-only correction for qualified NTFS disposable fixtures. Unsupported path types, filesystems, descriptor patterns, and existing-tree states return a typed fail-closed error. No persistent production schema or public protocol changes are allowed.

## 10. Required tests

- Exact C011 synthetic credential test: direct path, junction, hardlink, and ADS are denied by the child, or the request is rejected before child creation when the existing tree is not safely leaseable; approved workspace read/write remains covered by separate positive fixtures.
- Existing-file explicit allow, protected child DACL, nested directory, and multiple-file fixtures.
- Reparse entry inside a denied tree is rejected before child creation.
- Hardlink identity alias, path replacement, descriptor drift, inaccessible child, bounded tree-size exhaustion, and overlapping lease contention.
- Inject failures after every journal and ACL mutation stage; prove rollback or retained conflict without unjournaled changes.
- Restart/recovery on the hosted Windows runner; verify exact before/after DACLs for every touched object.
- C008–C011 Windows regression suite, Windows runner privilege probe, `cargo fmt --all -- --check`, `git diff --check`, and `bash scripts/verify.sh quick`.

## 11. Required verification commands

Hosted Windows:

`cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_sandbox_adversarial -- --nocapture --test-threads=1`
`cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_acl_lease -- --nocapture --test-threads=1`
`cargo test --locked -p codegg --features windows-sandbox-test-support --lib windows_acl_lease -- --nocapture --test-threads=1`

Cross-host:

`cargo fmt --all -- --check`
`git diff --check`
`python3 scripts/check_sandbox_contract.py`
`python3 scripts/check_execution_ownership.py`
`bash scripts/verify.sh quick`

Record exact runner build, Rust target, run IDs, tests skipped, and every failed/corrected attempt. No passing run may be attributed to a different implementation SHA.

## 12. Documentation updates

Update `architecture/security.md`, `architecture/testing.md`, `docs/windows-acl-leases.md`, C011's additive closure evidence, the Windows sandbox roadmap, and `plans/registry.md` only where implementation evidence changes the contract.

## 13. Acceptance criteria

- The exact C011 synthetic direct, junction, hardlink, and ADS reads are all denied, or the request is rejected before child creation when its existing tree is unsupported.
- Approved workspace read/write behavior remains intact in its positive fixture.
- Every changed ACL restores exactly after success, injected failure, cancellation, and recovery; descriptor drift is retained as a conflict.
- Unsafe/unbounded existing trees fail closed before the child starts.
- No production Windows filesystem capability or M004 closure is inferred from this test-only correction.

## 14. Stop conditions

Stop and retain C011/M004 as blocked if a safe tree lease would overwrite unrelated ACL state, cannot recover all partial mutations, races untrusted path replacement, allows any denied read, or exceeds bounded resource limits. In that case produce an ADR-backed future design proposal only if the existing authority does not answer the choice.

## 15. Closure evidence required

Same-revision hosted run and exact logs; DACL/identity trace for the synthetic tree; negative and positive process traces; per-object restoration/recovery results; conflict and fail-closed cases; exact unqualified scope; and an audit of whether C011 or M004 requirements can now advance. M004 remains blocked absent complete independent C011 review.

## 16. Handoff notes

Use disposable runner-owned NTFS directories and synthetic credential strings only. Do not access repository or runner secrets. Do not enable or advertise production Windows filesystem containment.
