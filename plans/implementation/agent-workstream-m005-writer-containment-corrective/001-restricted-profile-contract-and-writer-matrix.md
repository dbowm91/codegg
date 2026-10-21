# M005 Writer-Containment Corrective C001 — Restricted Profile Decision and Writer-Path Matrix

Status: closed — design/negative-test handoff only; cannot authorize shared execution
Primary class: invariant / cross-subsystem decision contract
Planning baseline: plan/agent-workstream-m005-writer-audit inspected 2026-10-19; ancestor main 61cbe9222c442c443cc7dfb3bd3a5b7d99ecd649.
Source: plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md C001
Original incomplete milestone: plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md (audit Q0 and Q1–Q5).
ADRs: accepted ADR-0002/ADR-0032; ADR-0047 was proposed at C001 time and was **accepted (option B) on 2026-10-19**. Canonical: plans/000-long-term-specification.md §§9, 16–17, 26–29; plans/001-terminology-and-domain-model.md §§5, 7–8; planning governance plans/003-planning-process.md.

## Delivered by

- Contract deliverable: `c001-restricted-profile-contract.md` — the 48-root writer matrix, the policy matrix, the user-visible guarantee and exact refusal contract, the lock-order and ownership model, the C002 protocol shape, and the twelve named holes.
- Disposition manifest: `docs/writer-disposition.toml`.
- Guard: `scripts/check_writer_disposition.py` (wired into `scripts/verify.sh quick`, with `--self-test`).
- Negative guards: `tests/writer_disposition_guards.rs`.
- Source audit evidence: `c001-evidence/`.
- Closure record: `plans/closure/agent-workstream-m005-writer-containment-corrective/001-status.md`.

Decision outcome at C001 close: **ADR-0047 was still proposed.** C002 remained blocked on an independent maintainer accept/reject decision.

**Subsequent decision (2026-10-19, after C001 closed).** The maintainers
accepted **ADR-0047 option B** (the restricted CodeGG-managed coordination
profile), recorded in the ADR's own "Acceptance record" section. That
acceptance is the independent decision C001's gate required, and it unblocks
C002. It does not retroactively change C001's outcome, and it does not enable
shared-checkout mutation by itself.

## Objective and defect

Original M005 inventory names many writer surfaces but there is no accepted product boundary or single authoritative capability matrix. A request-scoped lock cannot protect child descendants, PTYs or LSP/MCP; no existing permit covers editor, HTTP, project init, restore or CodeGG-local writes. This milestone establishes an explicit *decision-ready* restricted profile and a verifiable, maintained map of actual CodeGG writer sinks; it does NOT expose a new shared-write mode.

## Work packages

A. Reconcile source audit against branch HEAD: iterate ToolBroker effectful catalog, src/agent/tool_batch.rs, src/tool/task.rs, src/scheduler/executors.rs, ManagedProcessService, interactive PTY, src/git_mutations.rs/recovery/network, src/core/daemon_documents.rs, src/document_service.rs, src/lsp/mutation.rs, src/server/routes/file.rs, src/core/daemon_project_init.rs, snapshot restore, src/plugin, src/mcp, src/hooks, Tool Programs, src/worktree and project-local .codegg/runs/legacy DB. Distinguish executable sinks from read-only and private scratch. Match docs/execution-ownership.toml against code; it inventories process owners but is not a complete filesystem mutation index by itself. Record each reachable daemon/CLI/HTTP/frontend caller and final effect owner.

B. Define policy matrix for CodeGG-managed writes (not independent external same-UID programs): allowed proven read, candidate fenced native file/Git, denied process-backed/persistent, codegg non-broker fenced-or-denied, and unsupported platform/node/NonGit. Include effective ancestor tool permissions, sandbox, approval and principal ceilings; distinguish a readonly tool category from a server that can write. Explicitly deny shared placement absent accepted ADR/qualified permit.

C. Decide the user-visible guarantee, exact unsupported capability errors and operational fallback. Propose ADR-0047, surface its scope caveat prominently; do not mark it accepted in code or registry. Require an independent maintainer approval/rejection decision in C001 closure and correct the ADR document only while proposed.

D. Add focused negative/assertion fixtures or a narrow static inventory guard that would fail when a new model/direct/HTTP/daemon writer route lacks a profile disposition. A guard must not falsely claim dynamic shell contents are enumerable. No universal command regex blocker. Existing isolated runs retain full established tool access.

E. Document lock-order and ownership diagram (permission/approval first; job admission then effective permit; never wait on children while holding writer), plus migration/protocol shape for successor C002. This is a contract document, not authority to create any writer permits.

## Invariants / non-goals

No code path may be mislabeled isolated because CodeGG held a mutex; do not change ToolBroker tool permissions, daemon auth, WorktreeService cleanup, SubAgentExecutor default, human shell semantics or WorkOrder M001 fail-close. Do not automatically refactor all process APIs or add a second scheduler. No process/sandbox concession in exchange for sharing.

## Acceptance tests and evidence

- One complete table per CodeGG writer root: final owner, checkout locator, authority path, lifetime, allowed/denied/fence disposition, test/guard, OS status, and adversarial bypass example.
- Exercise failure for a prospective shared mutator without authority/profile; legacy isolated mutator continues.
- Verify no hidden CodeGG API (editor save, HTTP POST, restore, init, run-local .codegg) omitted.
- Verify policy denies arbitrary shell/Python/plugin/PTY/LSP/MCP until native proof; ordinary group read does not grant mutation.
- Write an explicit decision outcome: accepted ADR-0047, rejected, or still proposed. If still proposed, C002 remains blocked.
- cargo fmt --all -- --check, scoped tests/guards, scripts/verify.sh quick, and source inventory checks as relevant. Do not fabricate actual test results.

## Stop / closure

Stop and seek a new ADR if the desired product promise includes independent same-user writer exclusion, since a daemon permit cannot enforce it. Do not start Q1 or mark M005 partially supported because the audit table is complete. Closure record: plans/closure/agent-workstream-m005-writer-containment-corrective/001-status.md with exact SHA, source-to-sink matrix, decision and approval provenance, negative tests, historical M005 gap, guards, and C002 unblock audit.
