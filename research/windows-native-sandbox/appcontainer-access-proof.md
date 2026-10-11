# M004 WP-A — AppContainer access proof

Status: experiment passed; production feasibility not yet qualified

Source plan: `plans/implementation/windows-native-sandbox/004-filesystem-token-and-acl-enforcement.md`

Harness: `research/windows-native-sandbox/appcontainer-access-proof.ps1`

Workflow: `.github/workflows/windows-process-launcher.yml`, step `AppContainer filesystem access proof`

Initial run: `https://github.com/dbowm91/codegg/actions/runs/38120151470`

Token-query follow-up revision: `d3ef73d`
Token-query run: `https://github.com/dbowm91/codegg/actions/runs/38152215289`
Token-query run result: successful; Windows workflow completed all selected steps in 12m 18s.

ACL-restoration run: `https://github.com/dbowm91/codegg/actions/runs/38155073513`
ACL-restoration run result: successful; the workflow completed in 13m 25s.

The first token-inspection attempt invoked `whoami` inside the AppContainer and
failed with exit 1 (`38152089849`). That method did not provide token evidence;
it was replaced with a direct `OpenProcessToken` /
`GetTokenInformation(TokenAppContainerSid)` query, which passed in the
follow-up run above.

Two later workflow runs, `38153289874` and `38153623828`, failed in the
filesystem proof step after adding disposable ACL restoration checks. Run
`38154658189` also failed in 1m 21s. Its public workflow annotation confirmed
that all ACEs matched, but Windows added the DACL
auto-inherited (`AI`) control flag when restoring an inherited temporary
directory ACL. The fixture now first freezes the two disposable test-root
ACLs, preserving their inherited ACEs as explicit entries, and only then takes
the before-image. Run `38155073513` passed the exact descriptor restoration
assertion after restoring the before-image.

## Question

Can a per-run AppContainer package SID provide a second DACL access check that
limits a process even when the ordinary user/group ACLs grant the invoking
user access?

## Fixture and method

On the ephemeral hosted Windows runner, the script created a uniquely named
AppContainer profile and four new directories beneath `RUNNER_TEMP`. It added
an inheritable package-SID read/execute ACE only to a synthetic read root and
an inheritable package-SID modify ACE only to a synthetic scratch root. It
created a synthetic secret under a separate root with no package-SID ACE and
attempted an outside-root write to another ungranted directory. It launched
the system `cmd.exe` using `STARTUPINFOEX` and
`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`, waited for each process, then
deleted the profile and fixture tree.

No existing user, workspace, credential, system, or repository ACL was
modified. The fixture's changes were confined to directories it created.
The follow-up queried `TokenAppContainerSid` from each child process token
and required an exact match with the SID returned by
`CreateAppContainerProfile` before accepting the access result.

## Observations

Run `38120151470` passed on Microsoft Windows Server 2025 Datacenter,
version `10.0.26100`, build `26100`:

| Attempt | Result |
|---|---|
| Child token package SID equals the created profile SID | passed in run `38152215289` using `GetTokenInformation(TokenAppContainerSid)` |
| Read file under root with package-SID read/execute ACE | allowed |
| Write under root with package-SID modify ACE | allowed |
| Read synthetic secret under root with no package-SID ACE | denied |
| Create file under sibling outside root with no package-SID ACE | denied |
| Delete unique AppContainer profile and temporary fixture | completed without reported error |
| Restore exact disposable-root ACL before-images | passed in run `38155073513` after freezing the fixture ACL bases |

The same Windows workflow completed its existing process launcher suite
(8 passed), suspended-target failure injection (1 passed), and shell/policy
suite (all selected tests passed) in run `38155073513`.

Microsoft documents AppContainer access as the intersection of traditional
user/group grants and AppContainer package/capability SID grants, with the
package SID required for protected resource access. The observed fixture is
consistent with that access model. It demonstrates that AppContainer merits a
production feasibility pass; it does not establish CodeGG sandbox enforcement.

## Limits and next work

This harness does not exercise the CodeGG `ManagedProcessService`, inherited
Job membership, suspended-target setup, the exact requested policy compiler,
existing directory ACL leases, token/query evidence, credential store or
daemon IPC, hardlinks, ADS, junction races, cancellation, forced termination,
profile/ACL recovery, or non-admin/privileged VM adversarial cases. It uses
synthetic files only. Windows constrained launches remain fail-closed.

Next M004 work should implement an isolated launcher experiment behind the
canonical process owner, first proving token identity, inherited Job
membership, child/grandchild behavior and the policy read/write matrix. Only
after that should temporary ACE ownership/recovery, real sensitive-path
fixtures, and the full adversarial VM matrix be evaluated. If those steps
cannot preserve preexisting security descriptors and fail closed on cleanup,
stop before backend registration.

References:

- [Microsoft: Launch an AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)
- [Microsoft: CreateAppContainerProfile](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-createappcontainerprofile)
