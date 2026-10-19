# M005 Writer-Containment Corrective C004 — Non-Broker Project-Root Writer Mediation (Q3)

Status: blocked — C003 strict closure (and upstream accepted ADR-0042)
Primary class: invariant / capability
Planning baseline: plan/agent-workstream-m005-writer-audit source audit 2026-10-19; revalidate before editing.
Roadmap: plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md C004
Original unmet requirement: plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md project-root sink inventory, Q3.
Canonical: plans/000-long-term-specification.md §§9, 16–17, 27–29 and plans/001-terminology-and-domain-model.md §§5, 7–8.
Decisions: accepted ADR-0002, ADR-0032 and explicit ADR-0042 (currently only proposed).

## Objective

Close the most easily missed security gap: native-model tool writers are not all of CodeGG's checkout writers. Every CodeGG-controlled entry point that can modify an *active shared-reserved checkout* must participate in the same C002 permit, or deny the operation before effects. The existence of a user session or human principal must not bypass concurrency protection. Preserve independent authorization and expected UX for ordinary isolated/non-shared sessions.

## Source evidence and surfaces

The M005 audit identifies src/core/daemon_documents.rs and src/document_service.rs for document save/delete/create, src/lsp/mutation.rs preview application, src/server/routes/file.rs HTTP file write/delete, src/core/daemon_project_init.rs AGENTS.md publication, codegg-core snapshot restoration and project-local .codegg/runs / legacy .codegg/sessions.db mutations. It also lists generic Git service paths, worktree admin, CLI/TUI/desktop code paths that may target the same root. Final effect owners, not frontend buttons, must be audited. src/ide/mod.rs temporary diff copies and read-only file-tree listing need no checkout writer permit if they truly cannot modify the root.

## Work packages

A. Repeat M005 sink-to-final-effect inventory against current HEAD and add deterministic coverage for each reachable route, including HTTP and frontend-neutral daemon request handling. Derive worktree reservation/generation from canonical workspace/project/checkout identity at effect time; linked worktrees with a .git file and alternate path aliases must map to same canonical identity. Reject stale, external/unsupported or absent scope before physical writes.

B. For each transactional CodeGG-native sink, route checked mutations through the shared C002 final writer service, preserving existing document revision leases, project authorization, atomic file semantics, rollback and Git policies. Establish lock-order with document/repository/write permits and ensure no synchronous blocking while holding locks that can deadlock the scheduler. Alternatively implement an authoritative pre-effect reservation check denying the operation while a shared group is active; this is the preferred narrow approach for rare admin/restore/project-init paths.

C. Protect human-originated CodeGG modifications (TUI editor, HTTP API, desktop/ACP paths) equally. Human authorization is independent: if an operation is denied due to shared checkout reservation it must return a useful typed conflict/recovery path, not an unsafe broad-approval exception. Do not silently release a group's permit or kill another user's run.

D. Project-local metadata: classify .codegg/runs and legacy sessions.db migration as write sinks when located under checkout; either rehome/exclude them by a documented stable store policy or mediate/deny while shared reservation exists. Protect startup/recovery paths that run before a model turn and aren't aware of run IDs. Do not move user data or delete dirty state on a presumed cache classification.

E. Add one source-backed writer-sink registry/guard or narrow fixture suite that reliably fails when a known CodeGG file API loses mediation. A static inventory of direct process sites cannot substitute for inspection of nested tools and backend effects. Keep audit events attributed but redacted.

## Acceptance matrix

- Two mutation sources alternate: AgentRun native file transaction versus HTTP write, editor save, LSP preview apply (host), snapshot restore, project init; never overlap, or the non-broker source receives a pre-effect refusal. Verify no file/index/branch/metadata change on denial.
- Other principals/projects cannot acquire another group's permit; own project human user cannot bypass a reserved checkout merely by a broader approval profile.
- Pending document lock and writer permit do not invert lock order or deadlock; cancellation/restart preserves revision and real dirty-file state.
- CodeGG .codegg/runs and legacy DB writes have an explicit verified disposition and test; no false "outside checkout" claim.
- Separate checkouts continue ordinary non-blocking API writes; readonly file tree and temp diff operations do not require writer authority.
- Include genuine integration tests through real CoreDaemon, HTTP route and temporary Git checkout, not only a unit-level guard. Run focused suites, fmt, quick, Clippy and applicable daemon/permission/scheduler/source-ownership checks.

## Stop / closure

Stop C004 if any CodeGG entry can write to a shared reservation without final mediation or pre-effect refusal, or if a human API would require silently overriding someone else's checkout lease. Keep the entire shared profile disabled until fixed. File plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md with per-sink source and test evidence, migration/lock-order/race/authorization matrix, exact commands and C005 unblocking.
