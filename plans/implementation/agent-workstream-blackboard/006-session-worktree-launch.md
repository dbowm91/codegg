# Agent Workstream Blackboard M006 — Isolated Session/Worktree Fork-and-Launch

Status: closed — 2026-10-19. Closure: `plans/closure/agent-workstream-blackboard/006-status.md`

Delivered: the durable, idempotent launch saga in `codegg-core`
(`session_launch`, storage layout v85), the typed worktree request with a base
ref and a target branch, the daemon-side `SessionLaunchService`, the
`CoreRequest::SessionLaunch` / `CoreResponse::SessionLaunch` protocol pair, the
`workspace_project_binding.worktree_id` binding that was never written, the
`codegg launch` CLI affordance, the `worktree.create` authorization for the
checkout half of a launch, the TUI surface (branch, ownership, fork ancestry and
attention state, with the create/reuse/fork actions), and a service-level
integration test against a real Git repository.

Not claimed: the product's checkout *creation* on a host with the contained Git
backend — this revision has no contained backend, so the creation always fails
closed here — and three of work package E's cases that need a fault injector the
codebase does not have. See `architecture/session_launch.md` and the closure
record.

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

Work packages A (the typed request), B (the authorization and the
project-scoped resolution), C (the recoverable materialization), D (the
TUI surface) and the CLI affordance are delivered. Work package E is
delivered for every case that can be exercised without a fault injector;
the three that cannot are recorded below as a named gap rather than
claimed. M006 is **closed** — see `plans/closure/agent-workstream-blackboard/006-status.md`.

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

### Delivered since the first pass

- **The `worktree.create` authorization** for the checkout half of a
  launch. `Daemon::authorize_launch_checkout` runs *before* dispatch and
  before the allow event is recorded, so a denied launch has no side
  effect at all. A launch that reuses an existing managed checkout
  allocates nothing and therefore does not ask for the capability.
- **The project scope for a launch.** `resolve_authorization_project` had
  no `SessionLaunch` arm, so the `DirectProject` scope resolved to
  `None` and a team principal's launch was denied for a *missing scope*
  rather than for a missing grant.
- **The checkout's workspace is bound to the launch's project.**
  `ProjectStorage::bind_checkout_workspace` replaces the
  auto-provisioning path, which gave the checkout's root its own logical
  project and then failed the launch at the last step, after the checkout
  was already on disk.
- **The launch resumes and it leaves a visible record.** A failed launch
  advances its own record to `needs_attention` with a bounded category
  and cleans nothing up. A resumed launch continues from the stage the
  record reached; the fork's child id is derived from the launch id, so a
  retry converges on the child the first attempt made.
- **The TUI surface** (work package D). The session dialog shows each
  session's checkout — the branch, whether it is a CodeGG-managed
  checkout or the project's own root, and whether it needs an operator —
  and the fork's ancestry by name. It exposes the three actions: `n`
  places a new session in a new checkout, `w` places a new session in the
  selected session's *existing* checkout, and `F` forks the selected
  session into a new checkout. The plain `f` fork is untouched.
- **A service-level integration test** against a real Git repository:
  `tests/session_launch_service.rs` drives the service through a real
  `git worktree` in a real repository, and asserts on the linked
  worktree's `.git` *file*, the branch, the canonical bindings, the
  worktree count, and the failures.

### The contained Git backend

`git worktree` requires **contained** execution
(`egggit::process::classify_git_request` → `CleanSmudgeFilter`), and this
revision has **no contained Git backend**: a contained request becomes
`BackendUnavailable` on both the async and the sync boundary, and
`WorktreeService::create_reserved` goes through the sync one.

So on this host the product's own checkout creation **always fails
closed**. That is the plan's own anticipated condition — *"If the
available Git backend cannot safely create a managed checkout on a
platform, the request must fail closed and retain recoverable state"* —
and it is implemented and proven: the launch refuses with the backend's
own error, creates no session, leaves a `needs_attention` record, and
marks the checkout `orphaned` / `git_error` for an operator.

The **placement** semantics are proven against a real linked worktree
that the test fixture creates with a plain `git worktree add`. The
product's checkout *creation* on a host with the contained backend is
**not** claimed.

### The named gap

Three of work package E's cases need a **fault injector** the codebase
does not have: a post-create SQLite failure, a cancellation between the
Git registration and the session bind, and a symlinked checkout path.
They are not claimed, and not simulated by a fake that proves nothing
about the filesystem. The properties they cover are argued in
`architecture/session_launch.md` ("The narrow windows") instead, and the
two *observable* halves of the same properties — a failed launch leaves a
visible `needs_attention` record, and a launch never cleans up — are
tested directly.

Two further cases are platform-scoped and are marked rather than run:
Windows path-length handling and the linked `.git`-file case are
exercised on Linux (the `.git` file assertion), and the Windows-specific
half is not claimed.
