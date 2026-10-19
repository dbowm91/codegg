# Agent Workstream Blackboard M006 — Isolated Session/Worktree Fork-and-Launch

Status: in progress — the durable launch contract and the CLI affordance are
delivered; the TUI surface is outstanding

Delivered so far: the durable, idempotent launch saga in `codegg-core`
(`session_launch`, storage layout v85), the typed worktree request with a base
ref and a target branch, the daemon-side `SessionLaunchService`, the
`CoreRequest::SessionLaunch` / `CoreResponse::SessionLaunch` protocol pair, the
`workspace_project_binding.worktree_id` binding that was never written, and the
`codegg launch` CLI affordance.

Outstanding: the TUI surface (branch, ownership, fork ancestry and attention
state), the `worktree.create` authorization for the checkout half of a launch,
and a service-level integration test against a real Git repository. See
`architecture/session_launch.md`.

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

## Delivery status 2026-10-19

Work package A (the typed request), B (the authorization descriptor and the
project-scoped resolution), and the durable half of C (the stage machine,
the idempotency contract, the restart reconciliation) are delivered. The
CLI affordance is delivered; the TUI surface is not. M006 is **not
closed** — see the plan header for the outstanding work.

### Delivered

- `crates/codegg-core/src/session_launch.rs` — the durable launch saga:
  `SessionLaunchIntent`, `SessionLaunchPlacement`, `SessionLaunchStage`,
  `SessionLaunchRequest::fingerprint`, and
  `SqliteSessionLaunchStore` with `accept` (idempotent on
  `(project_id, idempotency_key)`), `advance` (checked forward-only
  transitions), `load`, `list` and `reconcile`. The accept path is a
  read-then-write and therefore opens its transaction with
  `BEGIN IMMEDIATE`.
- `crates/codegg-core/src/session/schema.rs` — `migrate_v85` adds the
  `session_launch` table with a closed `stage` enum and bounded columns.
  `STORAGE_LAYOUT_VERSION` 84 → 85.
- `crates/codegg-core/src/worktree_service.rs` — `CreateWorktreeRequest`
  gained `base_ref` and `target_branch`. A base ref is resolved to an
  immutable commit at reservation time by
  `eggit::refs::resolve_base_ref`, which is deliberately separate from
  `resolve_commit` (that one refuses anything but a full object id
  because its callers use it as provenance).
- `src/core/session_launch.rs` — `SessionLaunchService`: reserve the
  checkout, create it, register and bind the checkout's workspace, create
  or fork the session, bind the session, and advance the saga at each
  step. A retry converges on the recorded launch.
- `crates/codegg-protocol/src/core.rs` — `CoreRequest::SessionLaunch` and
  `CoreResponse::SessionLaunch`. The response carries the concrete
  session, workspace and worktree ids, not an `Ack`.
- `src/core/daemon_sessions.rs`, `src/core/daemon_family.rs`,
  `src/core/daemon.rs` — the handler and its classification and
  project resolution.
- `crates/codegg-core/src/authorization/policy.rs` — the `session_launch`
  operation descriptor, scoped `DirectProject`.
- `crates/codegg-core/src/audit_instrumentation.rs` — `session_launch` is
  **instrumented** (mapped to the `session_create` audit action), not
  hidden in the uninstrumented list: it is a shared-state mutation.
- `crates/codegg-core/src/project_storage.rs` —
  `record_workspace_worktree` writes the `workspace_project_binding
  .worktree_id` column that every binder had left `NULL`, and
  `project_repository` resolves a project's repository and source
  workspace.
- `crates/codegg-core/src/session/store.rs` — `rebind_workspace` moves a
  forked session out of its parent's workspace; the session row and the
  canonical binding must agree.
- `src/main.rs`, `docs/cli.md` — `codegg launch`, with `--project`,
  `--fork`, `--worktree`, `--base`, `--branch` and `--idempotency-key`.

### Outstanding

- **The TUI surface** (work package D): the session surface must show
  the current branch, the owned/external worktree, the fork ancestry and
  any cleanup/attention state, and the TUI must expose create/select/fork
  actions. `/worktree` is still list-only.
- **The `worktree.create` authorization** for the checkout half of a
  launch. The descriptor carries `session.create`; the checkout half is
  not yet authorized separately, so the handler must add that check
  before a launch may allocate a checkout.
- **A service-level integration test** against a real Git repository. The
  store contract is tested (11 tests in
  `crates/codegg-core/tests/session_launch.rs`) and the submission
  boundary is tested, but the service's Git path — reserve, create,
  register, bind — has no end-to-end test yet.
- **Work package E's remaining cases**: simultaneous identical requests
  at the service level, two principals, invalid ref, conflicting branch,
  nonexistent checkout, symlink path, post-create SQLite failure,
  cancellation between Git registration and session bind, restart repair,
  clean vs dirty retained, and Windows path-length and linked `.git`-file
  handling.
