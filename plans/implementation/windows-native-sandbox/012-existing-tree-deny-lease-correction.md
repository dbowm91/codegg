# Windows Native Sandbox Corrective C012 — Existing-Tree Deny Lease Correction

Status: implemented

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

Correct the test-only Windows ACL lease behavior that let an AppContainer read a synthetic credential beneath a denied existing directory. This bounded pass selects the fail-closed option: reject every populated deny root before profile creation or child spawn, without traversing or mutating descendants. It does not diagnose the exact ACE ordering cause or implement recursive leases. Windows constrained execution remains unavailable.

This is a corrective of C011's failed security qualification, not a production-backend enablement plan. M004 remains blocked until all of C011's requirements pass and a separate additive requalification is accepted.

## 2. Why this milestone is ready / dependency gate

Hard: C011 produced same-revision Windows Server 2025 evidence in run `38217933472`: direct, junction, and hardlink reads returned the synthetic credential contents despite the credential directory being declared denied; the alternate data stream read was denied. C010's owner-bound test-only lease/recovery infrastructure is available. The run `38217662482` at implementation revision `657587b9` passed the newly added final-component reparse-root rejection test and the rest of its Windows workflow.

## 3. Current implementation evidence

- `run_windows_test_restricted` leases DACLs on the root directories in `read_paths`, `write_paths`, and `deny_paths`.
- `windows_acl_lease::add_appcontainer_ace` adds inheritable AppContainer ACEs to a directory DACL. Existing files can also have their own effective DACLs and explicit ACE ordering; root-only success is not proof that existing file objects deny access.
- The C011 test used only runner-created directories and synthetic credential contents. It did not read repository, runner, or user secrets.
- `validate_empty_deny_directory` rejects a deny root containing any entry. The managed-process path performs this preflight before AppContainer profile creation and child spawn. The exact C011 DACL cause is not diagnosed; this correction does not infer it from root-only snapshots.

## 4. Invariants that must not regress

- A denied existing file remains unreadable through its original path, any junction/reparse route, hardlink, and alternate data stream.
- No child resumes until every requested restriction has been independently verified or the request fails with a typed unavailable result.
- Journaling precedes every DACL mutation; recovery restores only objects whose identity and expected descriptor still match.
- No broad user, group, or system ACE is removed or weakened. Third-party ACL drift is preserved as a conflict.
- The entire lease engine stays test-support-only. Production backend admission remains disabled.

## 5. Scope

### In scope

- Reject every populated deny root before child creation, without inspecting file contents, traversing descendants, or mutating ACLs.
- Verify that empty deny roots deny child read/list/write operations and restore their exact root ACLs.
- Verify that a populated synthetic deny tree fails before a marker-producing child starts and that its ACL snapshots remain unchanged.

### Explicitly out of scope

- Production `SandboxRequest::Required` backend, user workspace adoption, real credentials, daemon IPC, named-pipe qualification, UNC support, network controls, and M004 closure.
- Broad ACL normalization, taking ownership, changing unrelated ACEs, or weakening user-visible availability failures.

## 6. Required implementation changes

- Validate deny roots before AppContainer profile creation and child spawn; return typed `SandboxFailed` for populated roots.
- Keep ACL lease and recovery behavior limited to eligible empty disposable roots; do not claim descendant protection.
- Add a negative test asserting a populated tree fails before child start, with ACL snapshots unchanged.

## 7. Ordered work packages

### WP-A — Bounded fail-closed policy

Reject populated deny roots before any child creation or ACL mutation. Do not traverse the tree or read synthetic credential contents.

### WP-B — Native verification

On Windows Server 2025, prove populated-tree rejection happens before a marker-producing child starts and leaves ACLs unchanged. Separately prove empty-root deny behavior, positive allowed-root behavior, and exact lease restoration.

## 8. Failure, cancellation, restart, and contention semantics

Bound the transaction and traversal. A failed preflight or partial mutation must prevent child creation. Recovery restores only when all changed objects match their journaled identity and expected leased descriptor; otherwise retain the records and report conflict. Concurrent leases for overlapping trees must serialize or fail before mutation. Never release the lease before the Job and descendants have terminated.

## 9. Compatibility and migration

This remains a Windows-only, test-support-only correction for qualified NTFS disposable fixtures. Unsupported path types, filesystems, descriptor patterns, and existing-tree states return a typed fail-closed error. No persistent production schema or public protocol changes are allowed.

## 10. Required tests

- Populated synthetic deny tree, including junction/hardlink/ADS fixtures, is rejected before child creation; ACL snapshots remain unchanged.
- Empty deny root denies child read/list/write; allowed-root positive behavior remains covered; exact root ACL restoration is checked.
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

Same-revision hosted run and exact logs; populated-tree pre-spawn rejection; positive empty-root and allowed-root traces; exact ACL restoration; cross-host verification; exact unqualified scope; and an audit of whether C011 or M004 requirements can now advance. DACL root-cause diagnosis, recursive protection, restart fault injection for populated trees, and full C011 qualification are explicitly deferred and must not be claimed. M004 remains blocked absent complete independent C011 review.

## 16. Handoff notes

Use disposable runner-owned NTFS directories and synthetic credential strings only. Do not access repository or runner secrets. Do not enable or advertise production Windows filesystem containment.
