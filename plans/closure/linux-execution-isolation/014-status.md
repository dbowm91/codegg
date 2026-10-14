# Linux Execution Isolation C006 — Closure Status

Status: **blocked**

Source implementation plan:

- `plans/implementation/linux-execution-isolation/014-m003-protected-mount-view-and-git-metadata-backend.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `7cd4c469` (after C008 and command M004 test-only closures).

Implementation commits: none. Production M003 backend work is not authorized while the Linux mechanism ADR is proposed.

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
