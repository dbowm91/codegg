# Managed Scratch Native Qualification C002 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/managed-agent-scratch-native-qualification-corrective/002-windows-required-scratch-native-proof.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md#7-milestones`

Related independent roadmap (owner of the blocking dependency):

- `plans/subsystems/windows-native-sandbox-roadmap.md`

Repository baseline reviewed: `e1e8dd341e1f416ff7c713174c3144dd21da8c66` on `plan/managed-scratch-spaces`

Implementation commits or pull requests:

- None. C002 changed no production code: work package A of the source plan ends
  at "if absent, stop with blocked disposition", and that is the disposition
  reached. The sibling corrective C001 landed separately and is not a
  substitute for this milestone.

## 1. Executive finding

The milestone is **blocked**, exactly as its source plan predicted, and no
qualification work was started. Work package A requires an **accepted
Windows Required filesystem-enforcement backend contract** from the owning
Windows sandbox subsystem before any native proof may be written. That
contract does not exist: `plans/subsystems/windows-native-sandbox-roadmap.md`
still records M004 as blocked, records C011 as blocked on independent
privilege, IPC, descendant, restart and realistic-workspace qualification,
and states in its own status truth that "constrained Windows production
`SandboxRequest::Required` stays unavailable; no filesystem/network
`Enforced`". The product side agrees: `src/security/sandbox.rs` registers no
Windows filesystem enforcement backend, and `architecture/security.md` records
that on Windows "no filesystem or network enforcement backend is registered;
constrained requests fail before launch".

The blocker is therefore unchanged and external to this scratch workstream.
C001's Linux closure does not weaken it, and this record does not weaken,
rewrite, or reinterpret the owning roadmap's blocked findings — in
particular it does not turn `plans/closure/windows-native-sandbox/004-status.md`
from blocked into anything else.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP A: verify an accepted closure record and supported capability contract from the Windows sandbox owner | `plans/subsystems/windows-native-sandbox-roadmap.md` (status truth, M004 corrective sequence, C011/C015–C018/C030/C031 status lines) | fail — absent | No accepted Windows Required filesystem backend exists. |
| WP A: record backend guarantee, Windows build/runner, and effective token/process/ACL primitives | `plans/closure/windows-native-sandbox/004-status.md` and the C011–C038 corrective sequence | not run | There is no admitted backend to characterize. The owning roadmap records only test-only diagnostics and closed-negative experiments. |
| WP A: "if absent, stop with blocked disposition" | This record | pass | Stopped at WP A; no fixture, no test-only substitute, no speculative feature flag was written. |
| WP B: native Windows Required launch positive/negative scratch filesystem tests through the real `ManagedProcessService` | — | not run | Requires an admitted backend. Writing it against `windows-sandbox-test-support` would produce test-only evidence, which the source plan explicitly rejects as qualifying. |
| WP C: Windows reparse/junction/hardlink admission, cancellation and lease matrix | `plans/closure/windows-native-sandbox/013-status.md` (closed negative) | not run | Existing scratch lifecycle suites are regression evidence for reclamation, not for process containment. |
| WP D: execute the Windows hosted workflow and write supplemental C002 closure | — | not run | Nothing to execute; the added-native-test premise does not hold. |
| Invariant: "a passing Windows cleanup check is not evidence of OS-enforced write confinement" | `plans/closure/managed-agent-scratch/003-status.md`, `plans/closure/managed-agent-scratch/004-status.md` | pass | Preserved. This record makes no containment claim. |
| Invariant: "Required process fails before target launch if backend cannot enforce the requested scope; no implicit FullHost downgrade" | `src/managed_process.rs` fail-closed guard; `architecture/security.md` platform outcomes | pass (unchanged) | No change was needed; the product already refuses a constrained Windows request. |

## 3. Production implementation evidence

None, by design. The source plan's "Out" list excludes implementing a Windows
restricted-token/AppContainer/DACL containment architecture, and the owning
roadmap explicitly reserves that work for itself. The only production
surface touched by this corrective workstream is the Linux qualification
fixture recorded in the sibling C001 closure.

The Windows scratch **lifecycle** work that already landed is unaffected and
remains correct: `src/scratch.rs` allocates private and shared roots under
the per-user cache root, validates Windows reparse points, quarantines
reparse points during reclamation, and reclaims a revoked shared resource
only after the last attempt lease releases. That is directory lifecycle
hygiene under an inherited user ACL. It is not an OS access-control boundary
for a running child, and this record does not convert it into one.

## 4. Verification executed

### Commands run

```bash
# Plan dependency audit (read-only)
grep -n "M004\|Constrained Windows filesystem enforcement" plans/subsystems/windows-native-sandbox-roadmap.md
sed -n '1,25p' plans/subsystems/windows-native-sandbox-roadmap.md
ls plans/closure/windows-native-sandbox/
grep -rn "SandboxRequest::Required|filesystem|Required" src/security/sandbox.rs architecture/security.md
```

### Results

| Check | Result | Environment |
|---|---|---|
| Accepted Windows Required filesystem backend contract present | **fail (absent)** | local, read-only audit |
| Owning roadmap M004 status | blocked | local, read-only audit |
| Owning roadmap C011 status | blocked on independent privilege/IPC/descendant/restart/realistic-workspace qualification | local, read-only audit |
| Owning roadmap C015–C018, C030–C031 | blocked / terminally blocked | local, read-only audit |
| Product-side Windows filesystem enforcement backend registered | none | local, source audit |
| Windows native host available in this environment | none (development host is macOS) | local, environmental |

No Windows-hosted workflow was executed, because the source plan's premise —
an added native Required test in an existing lane — does not hold without an
admitted backend, and because no Windows host is reachable from this
environment. That is an environmental limit, recorded here rather than
implied.

## 5. Invariant review

- `ScratchId`, session/run owner and persisted immutable grants remain
  authority; paths and aliases do not create grants. **Unchanged.** No
  production code was modified, and the C001 fixture exercises the same
  typed-identity authority from the durable `AgentRunRecord`.
- Private `TMPDIR`/`TMP`/`TEMP`/`CODEGG_SCRATCH_DIR` never become globally
  writable shared temp. **Unchanged.**
- Required process fails before target launch if the backend cannot enforce
  the requested scope; no implicit FullHost downgrade. **Unchanged and
  still true on Windows** — a constrained Windows request is refused before
  launch, which is why this milestone cannot be satisfied by simply running
  the existing Windows launcher lane.
- Hard Deny and explicit tool/path Ask precedence preserved; reparse,
  junction and Win32/NT path aliases cannot bypass containment. **Not
  claimed.** This is precisely the unresolved surface the owning roadmap
  keeps blocked, with C011's hardlink-backed object mutation and descendant
  DACL control-bit drift still recorded there.
- Windows process-group/job-object supervision and admitted attempt lease
  outlive children; cleanup begins after the last lease. **Unchanged.**
  Lease and reclaim semantics are qualified for the Linux path by C001 and
  remain unproven for Windows process containment by this record.
- A passing Windows cleanup check is not evidence of OS-enforced write
  confinement or read confidentiality. **Preserved and reaffirmed.** The
  historical Windows workflow `39262822248` (11/11 scratch lifecycle) is
  explicitly *not* cited as containment evidence.

## 6. Failure and recovery review

- Unsupported backend: the product already refuses a constrained Windows
  request before spawn with a typed failure, and no child side effect
  occurs. This is the behavior the source plan requires, and it is why the
  milestone can be honestly reported as blocked rather than as a failure.
- Repeated authenticated invocations retain the original grants: unchanged;
  the C001 fixture additionally demonstrates that a retry of an accepted
  invocation cannot widen the frozen grant set.
- Job attempt active lease prevents scratch deletion; revoke stops new
  attempts without killing an existing active attempt: unchanged for
  Windows.
- Windows relaunch/restart recovery cannot claim a sandbox that was never
  obtained: unchanged, and the reason no recovery evidence is claimed here.
- No orphan lease was created by this milestone, because no attempt was
  admitted on a Windows host.

## 7. Migration and compatibility review

No schema migration, no protocol change, no configuration change, and no
default-enablement change. Storage migration v83 (managed scratch ownership
and lease data) is untouched. Existing Windows FullHost shell behavior,
ordinary shell execution, and the tested Windows scratch cleanup continue to
function exactly as before. No new semantic guarantee is promised on
Windows, and none is implied by this record.

## 8. Security review

- Authorization enforcement: unchanged. The typed `ScratchId` and the
  persisted immutable grant snapshot remain the only authority for a
  scratch root.
- Secret handling: no credential material was read, copied, or logged. The
  owning roadmap's prohibition on broad user-profile grants and on mutating
  original populated host ACLs is untouched by this milestone.
- Path validation: unchanged. No reparse, junction, or Win32/NT alias
  handling was added here; that surface remains owned and blocked by the
  Windows roadmap.
- Privilege boundaries: unchanged. No privilege was escalated, no host ACL
  or descriptor was modified, and no new production Windows execution path
  was introduced.
- Denial-of-service bounds: unchanged.
- Audit behavior: unchanged.

## 9. Documentation and operations

- No Windows-side documentation was changed, because the source plan permits
  documenting the native proof "only after owning backend is admitted".
- `architecture/security.md` and `docs/tools.md` were updated by the
  sibling C001 to record the **Linux** native qualification and to state
  that it covers the filesystem dimension on Linux only, and says nothing
  about a Windows host.
- Operator diagnostics: unchanged. The Windows constrained request continues
  to be refused with its existing typed error before any child is created.
- Static guards: `scripts/check_sandbox_contract.py`,
  `scripts/check_sandbox_policy_wiring.py`,
  `scripts/check_execution_ownership.py` and
  `scripts/check_scheduler_bypass.py` were run on the C001 head and pass.
  None of them can substitute for the missing native Windows evidence.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No admitted Windows Required filesystem-enforcement backend exists, so the delegated-scratch grant path cannot be qualified on Windows. | Windows constrained launches remain unavailable and fail closed. A user-visible Windows capability claim is not supported. | Owned by `plans/subsystems/windows-native-sandbox-roadmap.md` (M004/C011, and the C015–C018/C030–C031 blocked set). Resolve there; this plan stays blocked until an accepted backend contract exists. |
| medium | The Windows scratch **reclamation** evidence (workflow `39262822248`, 11/11) could be misread as containment evidence. | A reader could over-claim Windows scratch isolation. | Mitigated by this record and by the unchanged M003/M004 closure wording; keep the "cleanup is not containment" statement in `architecture/security.md` and `docs/tools.md`. |
| low | No Windows-hosted workflow was executed from this environment. | No fresh Windows CI run backs this record. | Not required while the milestone is blocked; a Windows run becomes required at the moment an admitted backend exists. |

No new finding was introduced by this milestone. The high finding is
pre-existing, external, and owned elsewhere.

## 11. Roadmap disposition

**Milestone blocked and reason:** the hard dependency — an independently
qualified Windows native **Required** filesystem sandbox backend — is
unmet, and the owning roadmap keeps it blocked. Per the source plan's stop
conditions, implementation was not started and no speculative,
feature-flagged release claim was made.

This closure does not unblock anything and does not require a corrective
implementation plan of its own: the work it would perform is exactly the
work the owning Windows roadmap is already registered to perform.

## 12. Registry updates

- `plans/registry.md`: the C002 entry moves from *blocked* to **blocked
  (closure recorded)**, pointing at
  `plans/closure/managed-agent-scratch-native-qualification-corrective/002-status.md`.
  The blocker text is unchanged: the Windows Required backend is absent.
- `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md`:
  C002's milestone row records the blocked closure and the unchanged
  dependency; the roadmap remains open because C001's counterpart closure
  and C002's blocker do not close the overall qualification track.
- **Unblock audit:** the registry's *Blocked work* section was re-read.
  This closure resolves no dependency, so no plan moves to *ready* because of
  it. The dependent Windows work is already registered in the owning
  roadmap and stays blocked there. C001's closure is the only plan this
  corrective milestone unblocks anything for, and it is recorded in the
  sibling C001 closure record.
