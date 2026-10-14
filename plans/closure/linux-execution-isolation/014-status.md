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
| C004 identity reconciliation | `plans/closure/linux-execution-isolation/012-status.md` | pass | ADR-0031 is uniquely identified; its proposed/accepted state did not change. |
| Explicit canonical Linux ADR acceptance | `plans/adrs/ADR-0031-composable-linux-execution-isolation.md` | pass (2026-10-14) | ADR-0031 is accepted. This clears the architecture decision gate only; it does not prove or enable a production backend. |
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

C006 moved to active implementation after ADR-0031 acceptance. The first source change rejects fixed-path Bubblewrap candidates with setuid or setgid mode bits. This narrows trusted-helper eligibility only; no mount backend was registered, no constrained launch routing changed, and no C006 enforcement or native denial evidence is claimed.

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

Before M003 can be retried, a separately reviewed design must provide an
enforceable policy for metadata created after launch-time discovery. Candidate
directions include a staged/copy-on-write workspace with validated promotion
that excludes executable Git metadata, or a kernel mediation mechanism that
can enforce path-component policy for newly created directories. Any successor
must preserve normal workspace edits, prove safe promotion and crash cleanup,
and pass the remaining native matrix. Static pre-scan, command-name filtering,
and mounting only currently existing `.git` paths do not satisfy this gate.

Registry disposition: C006 is closed as a bounded negative qualification;
M003 remains blocked, and nothing is unblocked. C007 and C008 retain their
prior closure records unchanged.
