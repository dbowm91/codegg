# Windows Native Sandbox Corrective C017 — Authorized Output Preview and Conflict-Safe Promotion

Status: blocked (C016 strict positive closure)

Repository baseline: `d00f1274c844bde7055559f240c020ffb2f1ff46` (`work/windows-native-sandbox-foundation`), 2026-10-12.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0019-projection-successor-milestones`

Accepted decision (experiment only): `plans/adrs/ADR-0019-windows-sandbox-staged-workspace-projection.md`.
Research basis: `research/windows-native-sandbox/adr-0019-successor-assessment.md`.
Inherited blocked security gate: `plans/implementation/windows-native-sandbox/011-m004-native-adversarial-qualification.md`; `plans/closure/windows-native-sandbox/011-status.md`; C013 `plans/closure/windows-native-sandbox/013-status.md`.

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Other applicable ADRs: `ADR-0004` approval/profile independence; `ADR-0016` platform-owned launches and truthful obtained guarantees.

Primary class: authorization / data consistency.

## 1. Objective

Define a reversible, audited **separate** output-promotion operation from untrusted projection back into the original authorized workspace. No implicit host modification by a sandboxed child, no privileged application of arbitrary staged paths, and no overwrite of modified or moved source objects.

## 2. Readiness and hard gates

Hard: C016 has produced restricted staged outputs and has demonstrated no direct host mutation. Existing Codegg workspace/worktree authority and approval routes remain canonical; no replacement Git service or frontend surface is authorized in this plan.

## 3. Repository evidence and security problem

A staged process result is untrusted. Codegg Git/worktree services (`src/git_service.rs`, `crates/egggit`, `src/scheduler/executors.rs`) offer domain operations but no blanket authorization for arbitrary snapshot patch promotion. A copied file may be based on stale/dirty host buffers or may be replaced with a junction/hardlink/ADS after staging; copy-back without source generation and explicit approval would violate the parent/subagent workspace ceiling.

## 4. Invariants and boundaries

- Windows production `SandboxRequest::Required` remains **unavailable**; experiment/test-only runs must not return `SandboxExecutionOutcome::Enforced`, update public capability or select FullHost.
- Never mutate real source workspace ACLs, SACLs, integrity labels, original hardlinks, user profile permissions or daemon named-pipe DACLs. No elevation or new account without a separate accepted design.
- `ManagedProcessService` owns restricted process creation, token/Job before resume, all descendants, stdout/stderr and status; authenticated daemon retains authorization and workspace ownership.
- AppContainer package SID, runtime permissions and file projection are *obtained* access facts only when the native tests prove them. No network isolation claimed without separate actual tests.
- Inputs are untrusted when copied into the projection; outputs are untrusted after the child writes them. Neither is a direct authority to modify the host worktree.
- All file paths and resource ownership must be bound to a stable workspace/worktree/job identity; stale paths, unsupported metadata, security conflict or partial setup fail closed.

## 5. Production-code boundaries and intended modules

Introduce internal `ProjectionOutputInventory` / `PromotionIntent` and source `ProjectionManifest` checks, with *read-only preview first*. The daemon/workspace authoritative operation applies bounded approved changes via existing lease/approval machinery or a narrowly reviewed adapter. Never trust target-produced paths to set destination authority; no new direct spawn or unreviewed `git apply` on host.

No unrelated UI, editor, human shell, distributed daemon, branch-wide merge or protocol refactor is permitted in this plan. Add public protocol/DB fields only after recording an explicit compatibility decision; tests and private types are preferred at this stage.

## 6. Ordered work packages

### WP-A — Inventory/preview
After Job teardown, enumerate only staged root using handle-bound path walk. Compare file hashes/identity to the immutable original snapshot, derive create/modify/delete candidates, preserve diffs as bounded metadata and preview. Reject reparse/hardlink/ADS/absolute path, `.git`, sensitive or excluded roots, case collision, reserved names and any output trying to cross original workspace boundary. No delete inferred from missing staged file unless explicit policy permits it.

### WP-B — Approval and authority
Bind promotion to original `WorkspaceId`, worktree lease/generation, project/session/human principal and explicit approval granted to **specific** content paths/operations. A successful process exit is not approval. Produce auditable attempt/receipt/result; enforce parent/subagent ceilings and deny automated FullHost conversion. Handle noninteractive mode as preview-only unless suitable prior authorized receipt exists.

### WP-C — Optimistic concurrency
Before each host write re-open actual destination and ancestor with reparse checks; compare original file ID/mtime/size/content digest and worktree generation to the snapshot. On conflicting user/editor changes, emit conflict and leave host file untouched. Record all newly created/modified objects and reject symlink/junction/hardlink swaps just before commit. Do not force overwrite, ignore case aliases or rename outside root.

### WP-D — Atomicity and restart
Apply through bounded, journaled operation with backup/staging and a clear atomicity model: per-file replace with rollback and explicit partial-apply status, or all-or-nothing where demonstrably feasible. Handle crash between review and commit, rename/replace and journal cleanup; never overwrite a third-party modified file on retry. Concurrent projections from same workspace serialize/optimistically conflict under the canonical worktree key.

## 7. Acceptance criteria

- Under preview-only path, actual host tree is byte- and ACL-identical before and after.
- An explicitly approved clean modification/new file is applied only to the authorized worktree; conflicting or changed originals are rejected without data loss.
- A staged output junction/hardlink/ADS/absolute path never causes an outside write, and denied user secret paths are never promotion targets.
- Cancellation/restart recovers owned temporary state without blindly reverting user edits or dropping audit provenance.

## 8. Failure, cancellation, recovery, concurrency and stop conditions

If original dirty editor state has no safe compatibility contract, **do not apply** and record deferred/preview-only. Never issue a model-generated approval. If C017 cannot reconcile without bypassing Codegg's canonical lease and authorization, stop and write an ADR or narrow interop corrective. Fail closed on filesystem/NTFS rename races, case aliases and journal conflict. Keep accumulated staged outputs bounded and expiring; user-controlled retention must not leak sensitive files.

## 9. Verification requirements

Pure tests: projection manifest diff, preview truncation, lease/revision conflict, idempotency, approval receipt and worktree owner mismatch. Native Windows new `windows_projection_promotion` integration: allowed create/modify, disallowed .git/secret edits, dirty destination, hardlink/junction/ADS/path race, crash and partial rollback. Re-run C014–C016 Windows tests, ownership/approval guards and `verify.sh quick`. Verify stdout cannot fake receipt.

All closure records must include exact SHA, runner/OS build, Rust version, workflow job link, **selected nonzero test counts**, actual child stdout/denied operation results, skipped native/privileged coverage and known regressions. A preflight refusal, compilation-only test, model statement or parent-generated status frame never substitutes for an actual running restricted-child security assertion.

## 10. Documentation and migration

Specify source hash/ID and staged artifact audit schema, bounded promotion journal/recovery, TTL, storage owner and any necessary private migration; no public protocol or UI until an explicit reviewed contract. Document compatibility with Git worktrees and editor dirty buffers, including preview-only fallback.

## 11. Closure record and handoff disposition

Positive C017 closure unblocks security qualification C018. Neither staged outputs nor an approved promotion alone establish Windows filesystem containment or unblock M004.

Write `plans/closure/windows-native-sandbox/017-status.md` with strict positive or negative evidence, separate from historical `004-status.md`, `011-status.md`, `012-status.md`, `013-status.md`. Update the registry and roadmap only on verified results. A negative feasibility result is a valid *closed-negative* plan, not a reason to mark an unsupported production feature ready.
