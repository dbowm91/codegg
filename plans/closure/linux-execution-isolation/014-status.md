# Linux Execution Isolation C006 — Closure Status

Status: **closed negative for strict M003 acceptance; production M003 remains blocked**

Source implementation plan:

- `plans/implementation/linux-execution-isolation/014-m003-protected-mount-view-and-git-metadata-backend.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `7cd4c469` (after C008 and command M004 test-only closures).

Reviewed-baseline implementation commits: none. Production M003 backend work was not authorized while the Linux mechanism ADR was proposed.

## 1. Executive finding

C006 remains blocked. At this record's reviewed baseline, ADR-0031 was proposed and the C004 identity cleanup had not accepted it. The C005 Bubblewrap evidence proves a bounded test-only namespace first hop; its closure explicitly does not qualify protected Git/credential mounts, production status ownership, or toolchain/PTY compatibility. M002 is conditionally closed for its typed interface/probe only and selects no new backend. The protected mount graph and Git metadata integration were not implemented or tested.

The separate maintainer decision was pending at the reviewed baseline; see the dated status update below.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| C004 identity reconciliation | `plans/closure/linux-execution-isolation/012-status.md` | pass | ADR-0031 was uniquely identified at the reviewed baseline; its proposed status did not change during C004. |
| Explicit canonical Linux ADR acceptance | `plans/adrs/ADR-0043-composable-linux-execution-isolation.md` | pass (2026-10-14; canonical ID later reconciled) | The Linux ADR was accepted as ADR-0031. This cleared the architecture decision gate only; it did not prove or enable a production backend. |
| C005 strict-positive native production launcher qualification | `plans/closure/linux-execution-isolation/013-status.md` | partial / blocked | Real test-only namespaces and setup-failure behavior passed, but protected overlays, production adapter integration, toolchain, Git workspace, and PTY compatibility were not qualified. |
| M002 typed launch contract | `plans/closure/linux-execution-isolation/002-status.md` | partial | Interface/probe slice is conditionally closed; no mount backend is selected and enhanced dimensions remain unavailable. |
| Protected workspace/Git metadata implementation | no implementation | blocked | No mount manifest, protected nested overlays, worktree alias handling, metadata lease, or trusted integration exists. |
| Native denial and compatibility matrix | not run | blocked | No protected-mount test was run; no new fixture or target is claimed. |

## 3. Implementation and verification evidence

No C006 production code or test fixture was added. No C006-specific test command was run. The predecessor records contain their own exact C004, C005, and M002 verification evidence; it is not reused as proof of C006 mount enforcement.

## 4. Invariant review

- The existing Landlock-only default and its filesystem-only reporting remain unchanged.
- No Bubblewrap/namespace backend was selected or registered.
- No writable workspace parent is represented as protecting nested Git or credential descendants.
- No network, process/IPC, seccomp, or resource guarantee is inferred from C005 or M002.
- No fallback to unrestricted Bash or trusted Git was added.

## 5. Unresolved blockers

| Severity | Blocker | Impact | Required action |
|---|---|---|---|
| high | C005 is test-only and does not qualify the protected mount view or production adapter. | Nested Git/credential protection and exact obtained filesystem status remain unproven. | Complete the actual mount/compiler/adapter design and strict-positive real-child matrix or stop with a corrective. |
| high | Git executable metadata writes and trusted Git effects have no protected-metadata lease. | Hooks/config/filters may still bridge untrusted writes into trusted execution. | Implement and verify lease-scoped metadata handling before C002 admission. |
| medium | Toolchain, Git worktree, LSP/test and PTY compatibility under an isolated mount view is unknown. | No supported compatibility profile can be advertised. | Qualify these paths on the native backend candidate before rollout. |

## 6. Roadmap and registry disposition

C006 remains in the blocked dependency register. ADR-0031 acceptance cleared the architecture decision gate on 2026-10-14; qualifying production mount evidence, protected Git metadata handling, and compatibility evidence remain outstanding. No plan became ready from the ADR decision alone. This is a blocked status record, not a successful milestone closure.

## 7. Status update — 2026-10-14

The maintainer accepted ADR-0031. This supersedes only the pending-decision statements above; the reviewed-baseline findings and implementation evidence remain unchanged. At that point C006 remained blocked until the protected mount graph, production launch integration, Git metadata lease, strict-positive real-child denial matrix, and required compatibility evidence were implemented and verified. Acceptance did not select, register, or enable the Bubblewrap backend or change defaults.

## 8. Implementation-start update — 2026-10-14

C006 moved to active implementation after Linux ADR-0031 acceptance. The first source change rejects fixed-path Bubblewrap candidates with setuid or setgid mode bits. This narrows trusted-helper eligibility only; no mount backend was registered, no constrained launch routing changed, and no C006 enforcement or native denial evidence is claimed.

Current identity note (2026-10-15): current main uses ADR-0031 for accepted
authenticated idle continuation. The Linux decision referenced above is now
canonical ADR-0043 with unchanged substance and accepted status. This note
reconciles identity only; the findings and verdict remain unchanged.

## 9. Implementation and hosted native qualification update — 2026-10-14

C006 produced an internal, opt-in Bubblewrap protected-mount candidate through
`ManagedProcessService` and the trusted helper. The implementation compiles a
fresh-root mount graph, pins bind sources with `openat2`/fd binds, checks grant
identities, masks denied paths and discovered Git metadata, gates target start
on Bubblewrap setup status, and checks the terminal exec result. The existing
Landlock route remains the default; no production route or profile was
registered.

The hosted workflow records Ubuntu 24.04 x86_64, kernel
`6.17.0-1022-azure`, UID 1001, Bubblewrap 0.9.0, AppArmor profile `unconfined`,
and Bubblewrap mode 0755 (not setuid). The runner's
`kernel.apparmor_restrict_unprivileged_userns` was `1`; the dedicated
ephemeral job recorded it and set it to `0` before native execution. This was
needed only to exercise user namespaces on that hosted runner. The workflow is
bounded to 20 minutes and the test target uses a 15-second process timeout.

Hosted run `38536302274` passed on exact SHA `e9f0c2b750613bd2f1af41eed89f9b7df13969a6`:
the trusted helper built and
`protected_mount_view_real_managed_process_handoff` passed 1/1 in 0.54 seconds.
The child wrote an allowed workspace file, read but could not change the root
Git config/hooks, could not hardlink protected config into the writable
workspace, could not read/write existing or future denied secrets or an
outside symlink target, saw no host `/proc` or `/sys`, ran Python, and completed
identity and timeout fail-closed probes. Earlier hosted runs `38529346419`,
`38530810345`, and `38531874263` exposed the runner UID-map restriction, an
allowlisted `/etc/gai.conf` directory, and file mount destinations created as
directories; run `38532957219` exposed missing `/lib64` compatibility path.
Each issue was corrected before the passing runs `38534083804` and
`38535156777`; run `38536302274` verifies the final reporting correction.

## 10. Negative disposition and remaining M003 blocker

This closes C006's bounded implementation and native candidate qualification
with a **negative result for strict M003 acceptance**. Static mount discovery
protects existing nested `.git` entries and the missing workspace-root `.git`
path. It cannot protect a `.git` directory created later beneath a new
writable subdirectory. That directory is backed by the direct writable
workspace bind and is absent from Bubblewrap's static mount graph. This meets
the plan's explicit stop condition for an unprotectable nested descendant.

The plan's remaining hard gates were not met: linked worktrees and external
common-dir aliases, concurrent rename races, ordinary Cargo/rustup, LSP/PTY
compatibility, startup timing, a complete descriptor-inheritance matrix, and
the typed non-activated Git metadata transaction contract were not qualified.
The passing 1-test native slice is not M003 production approval. No M003,
M004/M005 or Git C002 enforcement is claimed; the Landlock-only behavior stays
canonical.

The 2026-10-14 trust-boundary review concludes that arbitrary nested `.git`
creation need not be forbidden if a new repository cannot redirect a trusted
daemon Git operation outside its selected worktree or introduce executable
metadata on a trusted route. `research/linux-execution-isolation/m003-future-git-metadata-trust-boundary.md`
records that design. `egggit::process::GitEnvPolicy` now pins the selected
worktree and isolates system/global config; native tests prove a hostile local
`core.worktree` cannot redirect trusted status and executable fsmonitor
metadata does not run. The C006 mount candidate remains unqualified for
production, and the other native/compatibility gates listed above remain open.
This update does not rewrite the reviewed-baseline evidence or change the
negative C006 disposition.

## 11. Exact reconciled-head hosted rerun — 2026-10-14

After reconciling the implementation branch with `main` at `39f149a1`, the
current branch head `aef5ceb068c3e8ebb2a83112f88fd346e84ea1c1` was qualified by
hosted run [38614527301](https://github.com/dbowm91/codegg/actions/runs/38614527301).
The runner was Ubuntu 24.04.5 x86_64, UID 1001, Bubblewrap 0.9.0 at
`/usr/bin/bwrap` with mode 0755, and AppArmor profile `unconfined`. The hosted
workflow recorded `kernel.apparmor_restrict_unprivileged_userns=1` and changed
it to `0` on the ephemeral runner for this qualification. The trusted helper
built successfully and
`managed_process::tests::protected_mount_view_real_managed_process_handoff`
passed 1/1 in 0.54 seconds (6093 filtered out).

This exact-head rerun supersedes the older hosted run as current-head evidence;
it confirms the bounded protected-mount candidate behavior, not production
backend readiness. The selected-worktree trusted-Git boundary documented above
means arbitrary nested `.git` creation is allowed as workspace content. It
does not close linked-worktree/common-dir alias handling, concurrent rename
races, Cargo/rustup, LSP/PTY, startup timing, descriptor-inheritance, or the
remaining native compatibility matrix. C006 therefore remains closed with a
negative strict-M003 disposition, and M003 remains blocked.

## 12. Final implementation-head hosted rerun — 2026-10-14

After the Windows-only managed-request destructuring fix and the Test-runner
cancellation regression, final branch head
`4c00d5b58e6bb2421163271192b0773c66eba11e` passed hosted run
[38618460206](https://github.com/dbowm91/codegg/actions/runs/38618460206).
The runner was Ubuntu 24.04.5 x86_64, kernel `6.17.0-1022-azure`, UID 1001,
Bubblewrap 0.9.0 at `/usr/bin/bwrap` with mode 0755, and AppArmor profile
`unconfined`. The runner initially reported
`kernel.apparmor_restrict_unprivileged_userns=1`; the workflow set it to `0`
on the ephemeral host for the qualification. The helper built successfully;
`managed_process::tests::protected_mount_view_real_managed_process_handoff`
passed 1/1 in 0.56 seconds (6094 filtered out).

This is exact-final-head C006 evidence for the bounded opt-in candidate only.
It does not register the Bubblewrap production backend, change Landlock as the
canonical default, close M003's linked-worktree/common-dir, concurrent-rename,
toolchain/rustup, LSP/PTY, startup-time, descriptor-inheritance, or broader
compatibility gates, or unblock downstream Git enforcement.

Registry disposition: C006 is closed as a bounded negative qualification;
M003 remains blocked, and nothing is unblocked. C007 and C008 retain their
prior closure records unchanged.

## 13. Final reconciled implementation-head hosted rerun — 2026-10-14

After the accepted-authority routing fixture correction, exact branch head
`24593079b9b38fc0cb066f33c30fd1477b663483` passed hosted C006 run
[38633625863](https://github.com/dbowm91/codegg/actions/runs/38633625863).
The runner was Ubuntu 24.04.5 x86_64, kernel `6.17.0-1022-azure`, UID 1001,
Bubblewrap 0.9.0 at `/usr/bin/bwrap` with mode 0755, and AppArmor profile
`unconfined`. The runner initially reported
`kernel.apparmor_restrict_unprivileged_userns=1`; the workflow recorded that
value and set it to `0` on the ephemeral host for qualification. The trusted
helper built successfully, and
`managed_process::tests::protected_mount_view_real_managed_process_handoff`
passed 1/1 in 0.55 seconds (6094 filtered out).

This supersedes the earlier exact-head C006 run in section 12. It is evidence
for the bounded opt-in candidate only. It does not register Bubblewrap as the
production backend, change Landlock as canonical, or close linked-worktree and
common-directory aliases, concurrent rename races, Cargo/rustup, LSP/PTY,
startup timing, descriptor inheritance, or the remaining compatibility matrix.
M003 remains blocked and no Git C002/C003 or downstream Linux production plan
is unblocked.

## 14. Trusted-Git compatibility head hosted rerun — 2026-10-14

After adding the identity-only configuration handoff for trusted commit
operations, exact head `b0778262f48b50e3629fc9a239af15d3ceb5f20a` passed hosted
C006 run [38638600698](https://github.com/dbowm91/codegg/actions/runs/38638600698).
The Ubuntu 24.04.5 x86_64 runner reported kernel `6.17.0-1022-azure`, UID
1001, Bubblewrap 0.9.0 at `/usr/bin/bwrap` with mode 0755, and AppArmor
profile `unconfined`. The workflow observed
`kernel.apparmor_restrict_unprivileged_userns=1` and set it to `0` on the
ephemeral runner. The trusted helper built successfully and
`managed_process::tests::protected_mount_view_real_managed_process_handoff`
passed 1/1 in 0.54 seconds (6094 filtered out).

This exact-head rerun supersedes section 13 as current-source C006 evidence.
It preserves the bounded negative strict-M003 disposition: Bubblewrap is not
registered as the production backend, Landlock remains canonical, M003 remains
blocked on the documented alias/race/compatibility gates, and no downstream
Git enforcement plan is unblocked.

## 15. Current-main reconciled production-head hosted rerun — 2026-10-14

After merging current `origin/main` and retaining the trusted-Git boundary,
exact source head `a28523f5a9bf3e89a03d21929d8616417eb1022f` passed hosted C006
run [38650935114](https://github.com/dbowm91/codegg/actions/runs/38650935114).
The Ubuntu 24.04.5 x86_64 runner reported kernel `6.17.0-1022-azure`, UID
1001, Bubblewrap 0.9.0 at `/usr/bin/bwrap` with mode 0755, and AppArmor
profile `unconfined`. The workflow observed
`kernel.apparmor_restrict_unprivileged_userns=1` and set it to `0` on the
ephemeral runner for qualification. The trusted helper built successfully and
`managed_process::tests::protected_mount_view_real_managed_process_handoff`
passed 1/1 in 0.56 seconds (6095 filtered out).

This confirms only the bounded protected-mount handoff on the reconciled source
head. Bubblewrap is not registered as a production backend; Landlock remains
canonical. Linked-worktree/common-directory aliases, rename races, Cargo/rustup,
LSP/PTY, startup timing, descriptor inheritance, and the broader compatibility
matrix remain unresolved. M003 stays blocked; no downstream Git plan is
unblocked.

## 16. Current-head reconciliation rerun — 2026-10-15

After reconciling the feature branch with current `origin/main`, exact head
`5f1f998616f8727e4a84aea37ff6125884da4d19` passed hosted C006 run
[38795627184](https://github.com/dbowm91/codegg/actions/runs/38795627184).
The Ubuntu 24.04 runner used Bubblewrap 0.9.0 at `/usr/bin/bwrap` (mode 0755,
root-owned). The ephemeral runner began with
`kernel.apparmor_restrict_unprivileged_userns=1`; the workflow relaxed that
host-only gate to `0` for qualification. The trusted helper built, protected
mount qualification passed 11/11, and trusted-Git repository-config tests
passed 18/18.

This is current-head evidence for the bounded test candidate only. It does not
admit Bubblewrap for production or change Landlock as canonical. The explicit
C010 no-admission decision remains in force; M003 perf/compatibility gates and
production LSP/PTY routing remain open.
