# Windows Native Sandbox C024 — Dedicated-Principal Token and Host-Read Isolation

Status: closed (negative candidate result; no viable restricted-token child launch was established)

Repository baseline: `4d61f9622ee037313ce4781eb5b5666142d95a5d` on `work/windows-native-sandbox-foundation`; refresh HEAD before implementation.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-corrective-qualification-after-c022`.

Long-term requirements: `plans/000-long-term-specification.md` sections 4, 17, 27 and 29; `plans/001-terminology-and-domain-model.md` section 8.

Applicable ADRs: ADR-0004 (approval/profile), ADR-0016 (sandbox launch authority), ADR-0019 (historical AppContainer experiment), ADR-0020 (feasibility only).

Primary class: security invariant / infrastructure.

## 1. Objective

Determine whether a separate Windows principal can **actually deny reads of synthetic host objects accessible to broad user groups**, not just privately ACL-protected fixtures. Compare a standard dedicated user, `WRITE_RESTRICTED` token and an actually read-restricting token if technically viable. Close positive or negative after a bounded running-child matrix; no unsafe original-host ACL fixes.

## 2. Readiness

C019 common oracle and C021 bounded standard-principal evidence exist, and C022 retained fail-closed production unavailability. C023's launcher CI reconciliation may proceed in parallel; only C025 depends on both. Require a disposable administrator-controlled VM and explicit consent for account creation.

## 3. Evidence

`scripts/windows_dedicated_user_feasibility.ps1` calls `New-LocalUser`, `CreateProcessWithLogonW` suspended and Job assignment before resume. It **does not call** `CreateRestrictedToken`. C021 run [38334807679](https://github.com/dbowm91/codegg/actions/runs/38334807679) used synthetic host and sibling objects with private DACLs excluding that account; it proved denials and Git/Python/Cargo compatibility for that case, not default-deny reads of objects readable by `Users`, `Authenticated Users` or `Everyone`. C011 showed real alias reads under old mechanism; C013 proved modifying original populated ACLs unsafe.

## 4. Invariants

- A different `TokenUser` and `WRITE_RESTRICTED` do not by themselves enforce denied *reads*. Do not mislabel write confinement as full filesystem sandbox.
- No DACL/SACL/MIC, ownership, descriptor-control or hardlink-object mutation on existing workspace, profile, host fixtures or daemon IPC; synthetic stage ACLs may be owned/changed.
- No grants to the original user's credentials, home caches or arbitrary drive roots, no global `\Device\Null` changes.
- Test-only scripts and workflows; no production account creation, token runner, `Required` enablement, `Enforced` status, network claim or agent-admin setup.

## 5. Scope

Add a bounded candidate-only PowerShell/native helper and workflow (or extend C021 test fixture) to inspect effective token identity, groups/restricted SIDs, privileges and integrity. Test **private** and **broad-readable** original-host objects, including direct/junction/hardlink/ADS/extended-path aliases, sibling root, IPC, inherited handles, denied writes and required runtime tools. New **independent** stage file objects only; no copying original file security/links. Do not build output promotion or install a product helper.

## 6. Expected changes

A sibling `scripts/windows_dedicated_user_read_boundary.ps1` and test-only Windows workflow are preferred to avoid rewriting C021 historical fixture. Use Win32 `CreateRestrictedToken`/`CreateProcessAsUserW` or an appropriately authorized broker only if actual API privileges/semantics are verified. `CreateProcessWithLogonW` alone does not accept a prebuilt restricted token. Ensure suspended start, handle allowlist where possible, Job-before-resume, clear child/parent provenance. No production code, schema, protocol or MSRV change.

## 7. Work packages

A. Freeze threat/allowlist: origin host-secret and user-group ACE fixture, independently created allowed stage, original-workspace/sibling, operator pipe and handles. Snapshot original file IDs, link count, owner, DACL/SACL/control/MIC and contents.

B. Run the *current* separate-standard-user C021 path against synthetic `Authenticated Users`/`Users`/`Everyone` readable secret **and** private ACL control. A successfully prohibited read is a **negative baseline**, not an excuse to change the fixture's DACL.

C. Compare actually observed token modes: normal dedicated principal, `WRITE_RESTRICTED` (write semantics only), and restricting-SID mode if developer DLL/system reads can be granted narrowly without touching original populated roots. Capture `TokenUser`, groups, restricted SIDs, integrity and exact OS access failure. If no viable read restriction, close negative, no further ACL workaround.

D. Independently run native shell, Git init/status, Python real script, Cargo bounded no-network command and every C019-style direct/alias, outside-write, inherited-handle, named-pipe and descendant probe. Tool failures must not skip security probes when a child starts. Record exposed system/profile/registry/UNC/device exceptions honestly.

E. Verify all original host metadata and hashes unchanged, capture startup/cleanup failures, return strict positive / strict negative / blocked with exact SHA and Windows edition/UBR.

## 8. Failure/restart/contention

Child and Job must terminate before test-only resource teardown; no adopted foreign account or root. An access leak, unresolved token identity, required original host-wide ACL grant or unreadable essential toolchain is a negative stop. Host/privilege absence is blocked. Extended concurrency/reboot testing belongs to C025.

## 9. Compatibility

Test observed Windows Server and Windows 11 builds separately; no Windows Server 2025 result may claim a Windows 11 release matrix. No shipped configuration/protocol/credential changes.

## 10. Security tests

Broad group ACE host read **required for every candidate**; private ACL control; direct, hardlink, junction/reparse, case and extended paths, ADS; denied writes/create/move/delete; pipe, inherited handle, unauthorized sibling and stage isolation. Positive Git and shell minimum (Python/Cargo claims each require real pass); Job/descendant proof, before/after security descriptor/hardlink metadata and file hashes.

## 11. Verification

`git diff --check`; `cargo fmt --all -- --check`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`.

Run native `pwsh -NoProfile -File scripts/windows_dedicated_user_read_boundary.ps1` **if created**, plus C021 baseline `pwsh -NoProfile -File scripts/windows_dedicated_user_feasibility.ps1` on a consented disposable admin Windows VM. Capture actual child stdout/errors, nonzero selected probes, effective token and exact hosted/VM run URL. These new commands are targets, not claims they already exist.

## 12. Documentation

`research/windows-native-sandbox/adr-0020-harness-comparison.md`, roadmap/registry upon result, new `plans/closure/windows-native-sandbox/024-status.md`. Preserve original C021 closure.

## 13. Acceptance

Strict positive requires Git + shell running under a verified *read-constrained* child/Job, with actual denied reads of a synthetic broadly readable original host-secret through direct and aliased paths and denied writes/IPC/handles, no original object metadata changes. Otherwise close **negative** (if mechanism fundamentally fails) or blocked (host absent). Private-DACL-only evidence never counts as strict positive.

## 14. Stop conditions

Broad group-readable host read succeeds under every viable token, system compatibility requires original source ACL rewriting, token/group semantics unverified, child cannot execute minimum tools, or actor asks to relax host-secret guarantees. Do **not** spin further C024 variants indefinitely or unblock C025.

## 15. Closure

Exact SHA, OS/build/UBR/KB, token/group/restricted SID facts, per-operation native access matrix incl. failed/not-run counts, runtime tool compatibility, original metadata identity/descriptor evidence, teardown and final positive/negative/blocked result.

## 16. Handoff

**C024 is the hard security go/no-go for dedicated-user lifecycle investment.** If negative, skip C025 and use C026 to record no-admission, with MXC/VM as separate future architecture options.

## 17. Native token execution and broad-group access-check acceptance supplement

This supplement sharpens C024's existing strict read-enforcement gate. It is not a separate workstream or an instruction to modify the source user's DACLs.

**Known Windows contracts (official Microsoft documentation):**

- A true restricted token with restricting SIDs and **without** `WRITE_RESTRICTED` uses *two* access checks: enabled normal SIDs and restricting SIDs. Both must authorize the requested access; resource DACLs and token composition determine the actual result. `WRITE_RESTRICTED` applies the restriction list only to write accesses, so a successful write-denial test under that flag is not a read-denial proof. Source: https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken .
- `CreateProcessWithLogonW` authenticates the named account but has no parameter for the preconstructed restricted primary token. C021's current convenience launcher alone cannot run the read-restricted-token experiment. To run the actual restricted token, obtain a token of the **sandbox account**, create its restricted variant, then launch with an API such as `CreateProcessAsUserW`, with verified handle rights/privileges, appropriate user profile/environment and suspended Job assignment. Do not manufacture restriction by applying it to the privileged operator's token. Source: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw .
- A caller may require `SE_INCREASE_QUOTA_NAME` and possibly `SE_ASSIGNPRIMARYTOKEN_NAME`; `ERROR_PRIVILEGE_NOT_HELD` or an unavailable broker is a **blocked/negative experiment result**, not permission to fall back to unrestricted `CreateProcessWithLogonW`. A privileged broker may be evaluated **only inside the consented isolated test VM**, never a model-accessible/production setup path.
- Process/Job supervision has distinct semantics: a restricted process must enter the Job before `ResumeThread` and neither breakaway allowance may be set. The last privileged Job handle must not be inherited by the child, or kill-on-close may not fire on parent death. Source: https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects .

**Required discriminating test sequence (actual OS child, not synthetic AccessCheck alone):**

1. Create disposable source-file fixtures with verified `Users` / `Authenticated Users` / `Everyone` **read allow** ACEs. A **normal process as the dedicated standard account must successfully read** each eligible broad-readable fixture. In parallel, a file with a private DACL excluding that account must be denied. Record bytes, token identity and per-file security descriptors. This establishes that a later denial is not simply the ordinary account DACL.
2. Starting with the same sandbox-account token, create a non-`WRITE_RESTRICTED` token carrying a test-defined restrictive SID; show with `GetTokenInformation(TokenUser, TokenGroups, TokenRestrictedSids, TokenPrivileges)` that the launched child's actual primary token is the expected restricted principal. Record disabled groups, limiting SIDs and privileges. Prefer a per-run distinct limiting SID; verify any grant resides **only on newly created stage/runtime fixtures**, not the original source tree. A token query in the privileged parent is insufficient.
3. In that **running child**, request read access to the broad-readable original file through direct/alias paths and assert denial; independently assert permitted staged read/write and useful shell/Git operations. Distinguish `read data` from traverse/metadata access, and measure Windows executable/DLL/registry dependencies rather than granting blanket user-profile roots. An ordinary `WRITE_RESTRICTED` token may be exercised as a negative comparison; it must not be reported as the positive read-isolating mode.
4. Repeat with hardlinks sharing an underlying file ID, protected DACL children, reparse/junction traversal, ADS, case aliases, sibling workspace, pre-opened operator handle and named pipe. Record native Win32 error, child exit, observed file identity and before/after metadata. Unsupported namespaces/UNC must be labelled `not run` or `unavailable`, never `denied`.
5. The experiment must stop after one bounded causal token/launch comparison if child creation cannot preserve the required restrictions. Do not weaken read-deny semantics, modify original group-readable DACLs, assume `CreateProcessAsUserW` privilege availability, or continue by launching the unrestricted dedicated account instead.

**Gate implication:** positive C024 requires *both* the normal-dedicated-account **positive read baseline** and the true restricted-account-child **negative read result** for the same original file, alongside allowed stage/tool operations. Failure of the first baseline invalidates the experiment setup; failure of the second is a security-negative candidate finding. A token that cannot run the minimum toolchain or requires credential-bearing host grants is negative/incompatible. These states block C025; C026 records the no-admission disposition when appropriate.

**C025/C026 carry-forward:** C025's recovery journal must bind the restricting SID and actual child token identity to the owned account SID (not merely account name); C026's reviewer must reproduce the broad-readable baseline-vs-restricted-child contrast and inspect all primary-token launch and Job lifetime transitions.
