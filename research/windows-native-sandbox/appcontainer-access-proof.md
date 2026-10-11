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
ACL-restoration run result: successful; the workflow completed in 13m 30s.

Suspended-launch/Job proof: `https://github.com/dbowm91/codegg/actions/runs/38156232045`
The AppContainer proof step passed in 4s, including assignment of the suspended
target to its Job before resume. The full Windows workflow completed successfully
in 12m 56s: all 8 process launcher tests passed, suspended-target failure
injection passed, and all selected shell/policy tests passed.

Descendant Job proof: `https://github.com/dbowm91/codegg/actions/runs/38158022356`
The AppContainer proof step passed in 5s. An AppContainer `cmd.exe` started a
long-lived child; the child was observed in the same owner Job after the root
exited, then observed terminated after `TerminateJobObject`. The full Windows
workflow completed successfully in 14m 22s: all 8 process launcher tests,
suspended-target failure injection, and selected shell/policy tests passed.

Rust AppContainer token-query proof:
`https://github.com/dbowm91/codegg/actions/runs/38162151380`
The proof initially passed an undersized buffer to `GetTokenInformation`.
The corrected test queried the required size, allocated aligned storage, and
verified that the suspended child token contained the exact profile SID before
Job assignment and resume. The full workflow succeeded in 14m 14s: all 8
process launcher tests, suspended-target failure injection, the Rust
AppContainer token/Job proof, the disposable filesystem/ACL fixture, and the
selected shell/policy tests passed. The run used Windows Server 2025
Datacenter, version `10.0.26100`, build `26100`.

Access-control and shared Job-ordering proof:
`https://github.com/dbowm91/codegg/actions/runs/38164279173`
The fixture first read its synthetic secret from the ordinary runner principal,
then confirmed the AppContainer child could not read that same file. The Rust
AppContainer test exercised the same Job assignment/resume helper used by the
managed process path and verified `IsProcessInJob` inside the callback that
runs after assignment and before `ResumeThread`. The full Windows workflow
succeeded in 9m 59s: the fixture, all 8 process launcher tests, failure
injection, Rust AppContainer token/Job proof, and selected shell/policy tests
passed.

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
| Restore exact disposable-root ACL before-images | passed in runs `38155073513` and `38156232045` after freezing the fixture ACL bases |
| Assign the suspended AppContainer target to its Job before resume | passed in run `38156232045`; `IsProcessInJob` confirmed membership before `ResumeThread` |
| Preserve Job membership through a spawned AppContainer descendant | passed in run `38158022356`; the child remained in the owner Job after root exit and was terminated by `TerminateJobObject` |
| Rust `ManagedProcessService` suspended-target failure injection remains fail-closed | passed in run `38162151380`; injected Job assignment failure prevented the marker-writing target from running |
| Rust test-only AppContainer token has the exact package SID before resume | passed in run `38162151380`; the test queried the required token-information buffer size, compared `TokenAppContainerSid`, assigned the process to a Job, confirmed membership, and only then resumed it |
| Ordinary runner principal and AppContainer have different access to the same synthetic secret | passed in run `38164279173`; the runner principal read it, while the AppContainer child was denied |
| Rust AppContainer proof checks Job membership through the shared production assignment/resume helper | passed in run `38164279173`; `IsProcessInJob` ran after Job assignment and before `ResumeThread` |

The same Windows workflow completed its existing process launcher suite
(8 passed), suspended-target failure injection (1 passed), and shell/policy
suite (all selected tests passed) in run `38158022356`.

Microsoft documents AppContainer access as the intersection of traditional
user/group grants and AppContainer package/capability SID grants, with the
package SID required for protected resource access. The observed fixture is
consistent with that access model. It demonstrates that AppContainer merits a
production feasibility pass; it does not establish CodeGG sandbox enforcement.

## Limits and next work

This harness does not exercise the CodeGG `ManagedProcessService`, its
production suspended-target setup or inherited Job membership, the exact
requested policy compiler, existing directory ACL leases, crash recovery,
credential store, daemon IPC, hardlinks, ADS, junction races, cancellation,
forced termination,
profile recovery, or non-admin/privileged VM adversarial cases. It uses
synthetic files only. Windows constrained launches remain fail-closed.

Next M004 work should connect the proven suspended AppContainer + Job sequence
to CodeGG's canonical process owner without weakening its fail-closed behavior.
Then implement crash-recoverable owned ACE leases and test the actual requested
policy against real sensitive-path fixtures. The full adversarial VM matrix is
still required. If these steps cannot preserve preexisting security
descriptors and fail closed on cleanup, stop before backend registration.

## Evidence-record follow-up

`https://github.com/dbowm91/codegg/actions/runs/38165081918`
The same proof fixture and selected Windows suite passed again in 11m 17s.
The workflow completed the 8 launcher tests, suspended-target failure
injection, Rust AppContainer token/Job proof, and selected shell/policy tests.
This rerun validates the recorded evidence state; it adds no production
launcher or ACL-lease coverage.

References:

- [Microsoft: Launch an AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)
- [Microsoft: CreateAppContainerProfile](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-createappcontainerprofile)
