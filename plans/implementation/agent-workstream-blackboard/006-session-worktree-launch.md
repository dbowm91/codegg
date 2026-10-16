# Agent Workstream Blackboard M006 — Isolated Session/Worktree Fork-and-Launch

Status: ready for handoff — shared-write placement is explicitly out of scope

Planning baseline: 171ef93b6ba67d50b0412ae23b7697eb02d072eb (main, inspected 2026-10-16)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M006
Accepted authorities: ADR-0002, ADR-0004, ADR-0005. ADR-0032 is not required for this isolated-worktree scope.
Canonical: plans/000-long-term-specification.md §§6, 9, 13, 16, 25–29; plans/001-terminology-and-domain-model.md §§5–7.
Primary class: capability.

## Objective

Make isolated-worktree-aware sessions first class for CLI/TUI and daemon API, without changing what existing --fork and /fork mean. A user can start a new session, or fork conversation history, in a newly allocated isolated worktree or an already authorized isolated worktree; the session keeps the correct workspace/checkout/branch/root/base binding. Avoid ad-hoc frontend Git subprocesses. This milestone does not add shared checkout execution or shared-write leases.

## Existing code and gap

ADR-0002 remains the mutation-capable agent isolation contract. M005/ADR-0032 shared-write qualification is not a dependency for a session that owns a distinct checkout. The implementation must preserve existing WorktreeService lease generation and caller/project authorization. If the available Git backend cannot safely create a managed checkout on a platform, the request must fail closed and retain recoverable state; this limitation does not make shared-write containment a prerequisite.

SessionStore::fork in crates/codegg-core/src/session/store.rs copies recent messages and parent_id, preserving directory/workspace. src/core/daemon_sessions.rs CoreRequest::SessionFork performs session creation and rebinds the same workspace. docs/cli.md describes codegg --fork <id> and --cwd <dir>; TUI /worktree is list-only. WorktreeService owns managed checkout records and generation-fenced leases; their multi-step session orchestration is not yet one recoverable operation.

## Work packages

A. Typed request and UX contract. Define new daemon request(s) with explicit intent: new|fork session, placement: existing worktree ID or managed new checkout, optional base commit/ref and target branch policy, caller authority, idempotency key. Avoid ambiguous overload of SessionFork or paths-as-IDs. CLI examples: 'codegg session new --worktree <ref|new>' and 'codegg session fork <session-id> --worktree new' are indicative; select exact spelling consistent with current CLI parser and document old CLI compatibility. TUI exposes create/select/fork actions via existing project/session screens.

B. Precondition/authorization. Canonical project/repository/workspace selection, allowed existing external vs managed checkout, branch ref validation, parent context and principal scope must be checked in daemon. Child session may not switch into another tenant's checkout or gain extra sandbox/Git/tool rights. Managed checkout discovery is not ownership transfer.

C. Recoverable materialization. Design explicit durable stage transitions: request accepted -> checkout reserved/created -> workspace registered -> session created/forked -> session bound -> activation result. Each stage idempotently resumes on restart and duplicate call, or retains a visible orphan requiring operator attention. The service returns concrete new SessionId, WorkspaceId and optional WorktreeId rather than Ack followed by heuristic session-list selection. Never cleanup a dirty checkout on partial failure. Use transactional DB writes where possible; Git filesystem creation requires a repairable saga, not a false atomic multi-resource transaction.

D. Ergonomics. TUI surface displays current branch, owned/external worktree, fork ancestry and cleanup/attention state. Starting a session in an existing linked worktree must not create another one. New session remains cheap/default with no Git side effect; no forced prompt to allocate a checkout. Group-run placement and user session launch are separate owners.

E. Tests. Simultaneous identical requests; 2 callers with different principals; invalid ref; conflicting branch; nonexistent checkout; symlink path; post-create SQLite failure; cancellation between Git registration and session bind; restart repair; clean vs dirty retained; Windows path-length and linked .git-file handling; tests for both new/fork flows and unchanged old --fork semantics.

## Verification and acceptance

Focused SessionStore/daemon protocol/TUI/CLI/worktree/project binding/recovery tests; src/core/daemon_sessions.rs and CoreRequest compatibility, cargo fmt, scripts/verify.sh quick, workspace Clippy, core/protocol/TUI guards; evidence of real local Git linked worktree on supported host. Never claim a session is in a new worktree from transcript fork alone. No index mutation without canonical Git executor and approvals.

Stop if a request can bypass worktree lease generation, principal/project authorization, or silently clean/reuse dirty state. Shared checkout execution remains unavailable regardless of this milestone's status. Record executed commands, exact source SHAs, screenshots or deterministic TUI state evidence and user-facing acceptance in plans/closure/agent-workstream-blackboard/006-status.md.
