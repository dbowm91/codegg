# Managed Scratch Native Qualification C002 — Windows Required Sandbox Scratch Proof

Status: blocked — independently qualified Windows native Required filesystem backend unavailable
Repository baseline: `fb84bee63dda0b1c3407f377c695db99ec78324b` on `plan/managed-scratch-spaces`; recheck after upstream Windows sandbox closure.
Source roadmap: `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md#7-milestones`
Original gap: `plans/closure/managed-agent-scratch/003-status.md` conditional Windows **Required sandbox containment**, not Windows cleanup.
Long-term requirements: `plans/000-long-term-specification.md` §§4, 17, 26–29; `plans/001-terminology-and-domain-model.md` §§5–8; `plans/002-long-term-roadmap.md` Phases 9–11 and 19.
Related independent roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md` (filesystem enforcement and release qualification), accepted ADR-0001/0002/0028, plus the Windows native sandbox ADRs reviewed on execution.
Primary class: invariant.

## 1. Objective

After the Windows sandbox project provides a native **Required** filesystem-write enforcement contract, qualify that authenticated CodeGG session/child scratch grants are faithfully applied by real Windows processes and that ungranted sibling/host temp writes are denied, without confusing independently tested Windows scratch **reclamation** with process containment.

## 2. Why blocked

M001/M002/M004 have closed portable scratch behavior and Windows native scratch lifecycle already passed 11/11 on workflow `39262822248`. `src/scratch.rs` supports Windows shared reclaim and reparse-point quarantine. None of those results establish that Windows `SandboxRequest::Required` enforces a filesystem allowlist in product launches. `plans/subsystems/windows-native-sandbox-roadmap.md` still owns admission/obtained-native-backend/restricted-token/AppContainer/DACL research and closure; this scratch workstream **must not implement or promise that backend**. C001 Linux completion is not the blocker.

## 3. Current implementation evidence and prior missed verification

`src/scratch.rs` allocates private and shared roots, validates Windows reparse points and reclaims revoked shared resources. `.github/workflows/windows-process-launcher.yml` runs ordinary Windows process/scratch lifecycle and test-support restricted process fixtures. M003's original closure stated Windows cleanup disabled; its later factual correction and M004 closure supersede that historical description with tested Windows cleanup. No supported Windows Required managed-process sandbox end-to-end scratch admission/negative proof is present. A compilation, ordinary launcher smoke, or test-only Windows sandbox probe cannot close the remaining requirement.

## 4. Invariants

- `ScratchId`, session/run owner and persisted immutable grants remain authority; paths/aliases do not create grants.
- Private `TMPDIR`/`TMP`/`TEMP`/`CODEGG_SCRATCH_DIR` never become globally writable shared temp.
- Required process fails before target launch if backend cannot enforce the requested scope; no implicit FullHost downgrade.
- Hard Deny and explicit tool/path Ask precedence preserved; reparse/junction paths and Win32/NT path aliases cannot bypass containment.
- Windows process-group/job-object supervision and admitted attempt lease outlive children; cleanup begins after last lease.
- A passing Windows cleanup check is not evidence of OS-enforced write confinement or read confidentiality.

## 5. Scope

In after unblock: native Windows Required launch positive/negative scratch filesystem tests, reparse/junction/alias adversarial cases, obtained status and credential path boundary, background-lease/restart matrix, correction confined to integration seam if evidenced, closure/registry documentation.

Out: implementing Windows restricted token/AppContainer/DACL containment architecture, rewriting shell commands, broader Windows network isolation, new process launcher, remote scratch, new data storage or generic Windows runner coverage.

## 6. Required production changes

**Core/domain:** reuse M002 `AgentScratchScope`, typed `ScratchId`, immutable grant snapshot and existing JobSubmissionService → ManagedProcessService; no separate permission engine.

**Storage:** use existing v83 ownership/lease data; expect no migration. Test idempotency/owner validation before binding Windows handles to scratch roots.

**Protocol/DTO:** no new client raw path authority. Any obtained-capability status must come from existing sandbox result, not model fields.

**Runtime:** on a true Windows native Required backend, ensure roots in effective `SandboxLaunchSpec` are correct and obtained, not merely requested. Verify normal command execution and containment under `cmd.exe`/PowerShell as supported by current launcher; never accept hard-coded ambient TEMP exceptions.

**Security:** native handle/reparse/junction alias tests, sibling/parent/global temp write negative, credential deny and no unsafe grant widening. Sandbox absence causes deterministic refusal before side effects.

**Docs/guard:** update `architecture/security.md`, Windows backend docs and existing workflow tests only after owning backend is admitted. Preserve `scripts/check_sandbox_policy_wiring.py` and execution ownership invariants.

## 7. Ordered work packages

A. Verify accepted closure record and supported capability contract from Windows sandbox owner. Record backend guarantee, Windows build/runner and effective token/process/ACL primitives; if absent, stop with blocked disposition.
B. Add native Windows Required launch tests using **authenticated** session-run-grant context and actual `ManagedProcessService`, including positive private/shared and negative sibling/global temp/credential paths. Prove obtained status and no fallback.
C. Stress task/job admission, cancellation/lease release and safe shared reclaim under Windows reparse/junction/hardlink scenarios. Existing scratch lifecycle tests are regression evidence, not a substitute for native containment.
D. Execute existing Windows workflow and other guards on exact head; write supplemental C002 closure, update conditional roadmap/registry if and only if evidence justifies it.

## 8. Failure, cancellation, restart, contention

On unsupported backend, no child side-effect and a typed unavailable/failed enforcement result. Repeated authenticated invocations retain original grants, not environment-derived/new alias rights. Job attempt active lease prevents scratch deletion; revoke stops new attempts without killing an existing active attempt. Windows relaunch/restart recovery cannot claim a sandbox that was never obtained.

## 9. Compatibility/migration

Existing Windows FullHost, ordinary shell and tested scratch cleanup remain functional. No new semantic guarantee or default enabled without the upstream Windows roadmap accepting it. Preserve migration v83 and existing session snapshots.

## 10. Required tests

- Native **Required** process with actual obtained status: session-private and explicit shared write succeed, ungranted sibling/parent/host TEMP and credential path writes fail.
- Reparse/junction/alternate Win32/NT path negative test and direct-parent grant denial; property doesn't follow only from `Path::starts_with`.
- Shell environmental TMP/TEMP/private path, Windows powershell/cmd compatibility as actually supported.
- Active/retried/cancelled attempt keeps/releases leases and shared reclaim only after terminal process.
- Setup-failure sentinel proves no target execution without backend.
- Existing `scratch::tests` 11-case lifecycle and `windows_process_launcher` 8-case launcher suites remain green.

## 11. Required verification commands

```powershell
cargo fmt --all -- --check
cargo test -p codegg --lib scratch::tests --locked -- --nocapture --test-threads=1
cargo test -p codegg --test windows_process_launcher --locked -- --nocapture --test-threads=1
cargo test -p codegg --features windows-sandbox-test-support --test windows_restricted_managed_process --locked -- --nocapture --test-threads=1
cargo test -p codegg --features windows-sandbox-test-support --test windows_sandbox_adversarial --locked -- --nocapture --test-threads=1
```

And execute existing `Windows process launcher` hosted workflow with any added native Required test in an existing lane, plus supported cross-platform workspace format/Clippy/quick/static guards. **These existing test-only features are not by themselves qualifying evidence**; require actual product Required backend positive+negative execution.

## 12. Documentation updates

Append factual supplemental qualification to M003 closure only after successful native proof, preserving original date-specific failed assertions and M004 cleanup history. Reconcile `architecture/security.md`, `docs/tools.md`, current corrective addendum and registry; cite upstream Windows Required accepted closure and obtained host status.

## 13. Acceptance criteria

Windows production-path `Required` refuses when enforcement unavailable and, when the accepted backend is available, provides real process-local write enforcement of authenticated private/shared roots while denying ungranted sibling and host temp. Obtained status independently recorded. A test that substitutes FullHost must fail.

## 14. Stop conditions

Do not start implementation until Windows sandbox subsystem provides accepted Required filesystem backend interface and native evidence; no speculative feature-flagged release claim. Stop if scope would require an unaccepted new Windows sandbox design, broad TEMP allowlists or shell path rewriting, or if a sibling write succeeds. Track failures in owning backend roadmap and keep M003 conditional until corrected.

## 15. Closure evidence required

`plans/closure/managed-agent-scratch-native-qualification-corrective/002-status.md`: upstream backend acceptance reference, exact commit, Windows build and effective backend identity, authorization/grants/negative coverage, full native process results, cleanup/lease outcomes, CI and skipped tests, migration/security posture, residual severity, update/closure recommendation and unblock audit.

## 16. Handoff notes

Do not repeat Windows cleanup work already landed. Native workflow `39262822248` establishes launcher and scratch *lifecycle* tests, not Required sandbox. This plan should remain registered **blocked** until the separate Windows sandbox architecture is qualified. Mergeability of PR #128 is independent and requires no corrective plan.
