# M006 — Isolated Session/Worktree Fork-and-Launch — closure record

**Status:** closed
**Milestone:** M006 — Isolated Session/Worktree Fork-and-Launch
**Plan:** `plans/implementation/agent-workstream-blackboard/006-session-worktree-launch.md`
**Roadmap:** `plans/subsystems/agent-workstream-blackboard-roadmap.md` (M006)
**Closed:** 2026-10-19
**Branch:** `work/agent-workstream-m005-writer-audit`

## 1. What this milestone is

M006 makes **isolated** worktree-aware sessions first class for the CLI, the
TUI and the daemon API: a user can start a new session, or fork a
conversation, in a **newly allocated** managed checkout or in an **already
managed** checkout, and the session keeps the correct workspace, checkout,
branch and base binding.

It is not the shared-checkout milestone. Nothing here enables a shared
checkout, a shared mutation, or a second writer.

## 2. Why the request itself is durable

Launching a session into a checkout spans two resources that cannot share one
transaction: the **checkout** is a Git worktree on the filesystem, and the
**session** is a row in SQLite.

There is no atomic multi-resource commit, and the milestone does not pretend
otherwise. The *request* is recorded durably (`session_launch`) and advanced
through explicit stages. That record is what makes a retry converge: without
it, `WorktreeService::reserve` mints a fresh `WorktreeId` per call and
`SessionStore::fork` mints a fresh session id per call, so a retried launch
would allocate a **second** checkout and a **second** session.

## 3. What changed

### 3.1 The durable saga (`codegg-core`)

`crates/codegg-core/src/session_launch.rs`:

- `SessionLaunchIntent` (`New` | `Fork`), `SessionLaunchPlacement`
  (`NewManagedCheckout { base_ref, target_branch }` |
  `ExistingManagedWorktree { worktree_id }`).
- `SessionLaunchStage` — `accepted → checkout_reserved → checkout_ready →
  session_created → bound → completed`, with `needs_attention` as the
  terminal alternative. `SessionLaunchStage::rank` lets a resumed launch
  skip a stage it already passed, because the store's transitions are
  forward-only and a repeat is not a contract violation.
- `SqliteSessionLaunchStore` — `accept` (idempotent on
  `(project_id, idempotency_key)`), `advance`, `load`, `list`, `reconcile`.
  The accept path is a read-then-write and therefore opens its transaction
  with `BEGIN IMMEDIATE`.
- `SessionLaunchError::attention_code` — the bounded **category** recorded
  when a launch needs an operator. The category is a category, never the
  detail: the detail is the error's own message, and a code carrying it
  would end up in a durable record and in an operator's list.

`crates/codegg-core/src/session/schema.rs` — `migrate_v85` adds the
`session_launch` table with a closed `stage` enum and bounded columns.
`STORAGE_LAYOUT_VERSION` 84 → 85.

### 3.2 The checkout

- `crates/codegg-core/src/worktree_service.rs` — `CreateWorktreeRequest`
  gained `base_ref` and `target_branch`. A base ref is resolved to an
  immutable commit at reservation time; the record stores the commit, never
  the moving name. A target branch that already exists in the repository is
  refused as a collision.
- `crates/egggit/src/refs.rs` — `resolve_base_ref`, deliberately **separate**
  from `resolve_commit` (that one refuses anything but a full object id
  because its callers use the result as provenance). A worktree base is not
  provenance: it names where a checkout starts. The resolver is still narrow
  — `..`, `~`, `^`, `:`, `@{`, a leading `-` and any whitespace are refused.

### 3.3 The workspace binding — the defect the first pass shipped

The catalog's auto-provisioning rule gives every previously unseen workspace
root **its own logical project**
(`ProjectStorage::reconcile_workspace_path` → `resolve_project_and_repository`).
That rule is right for a directory the user opens and wrong for a launch: a
checkout is not a new project, it is a second working directory of the
project the caller already named.

The first pass used that path. It bound the checkout's workspace to a
*different* project than the session's, and then failed the launch at the
last step — `SessionStore::create_with_binding` requires the workspace's
binding to name the same project as the session — **after** the checkout was
already on disk. A launch that a user asked for could not complete.

`ProjectStorage::bind_checkout_workspace` replaces it: the checkout's
workspace joins the launch's project, shares the launch's repository, and
records the managed checkout it belongs to. It is refused when the workspace
is already bound to a *different* project. The write is a read-then-write and
therefore opens with `BEGIN IMMEDIATE`.

`workspace_project_binding.worktree_id` — the column every binder had left
`NULL` — is written by that binding, so a managed checkout can finally be
*named*.

### 3.4 The session

- `crates/codegg-core/src/session/store.rs` — `rebind_workspace` moves a
  forked session out of its parent's workspace **and project**; the session
  row and the canonical binding must agree.
- `SessionStore::fork_as(parent, child_id)` — a fork under a caller-chosen
  id, converging if that child already exists. The launch derives the
  child's id from the launch id, so a resume — or a retry — converges on the
  child the first attempt made instead of forking a second one. The id is
  *derived* rather than recorded, so the durable record never claims a
  session exists before it does.

### 3.5 Authorization

- `crates/codegg-core/src/authorization/policy.rs` — the `session_launch`
  operation descriptor, scoped `DirectProject`, carrying
  `Capability::SessionCreate`.
- `src/core/daemon.rs` — `resolve_authorization_project` gained a
  `SessionLaunch` arm. Without it the `DirectProject` scope resolved to
  `None`, and a team principal's launch was denied for a *missing scope*
  rather than for a missing grant.
- `src/core/daemon.rs` — `Daemon::authorize_launch_checkout` runs **before**
  dispatch and before the allow event is recorded. A launch that allocates a
  checkout is authorized for `Capability::WorktreeCreate` against the same
  project; a launch that names an already-managed checkout allocates
  nothing and does not ask for it. A denied launch therefore has **no side
  effect at all** — not even an accepted launch record — and the audit trail
  holds one decision, not an allow followed by a denial.
- `crates/codegg-core/src/audit_instrumentation.rs` — `session_launch` is
  **instrumented** (mapped to the `session_create` audit action), not hidden
  in the uninstrumented list: it is a shared-state mutation.

### 3.6 Recovery

- A launch that fails advances its own record to `needs_attention` with a
  bounded category and returns the error. **Nothing is cleaned up**: a dirty
  checkout and a half-made session are the evidence an operator needs.
- A resumed launch continues from the stage the record reached. The
  checkout is created only if the **worktree record** says it is still
  `reserved` — the worktree record, not the launch record, is the authority
  on whether the directory exists, because the two can disagree when a
  launch died between the two writes.
- Two **simultaneous** requests for the same launch are serialized by an
  in-process lock, and the record is re-read under it. The second caller
  converges on the first one's result. The durable saga is what survives a
  *crash*; the lock is what survives two *callers*.

### 3.7 The TUI surface (work package D)

`src/tui/components/dialogs/session.rs` and `src/tui/commands/sessions.rs`:

- The session dialog shows, per row, **where the session runs**: the
  managed checkout's branch, or `root` for the project's own working tree,
  and a `⚠` when the checkout needs an operator (an *orphaned* record with a
  `git_error` health is the shape a failed `git worktree add` leaves).
- The row shows the **fork's ancestry by name** — the parent's title, not
  the parent's id.
- The three actions: `n` places a new session in a **new** managed
  checkout, `w` places a new session in the selected session's **existing**
  checkout, and `F` forks the selected session into a new checkout. The
  plain `f` fork is untouched, so neither meaning silently takes the
  other's key. The hint line advertises all three.
- Starting a session in an existing linked worktree does **not** create
  another one: the reuse path names the checkout.
- A **new** session stays cheap and default: nothing in the ordinary
  new-session path allocates a checkout, and no prompt forces one.
- The checkout data comes from the same reload that fills the session list
  (`CoreRequest::ManagedWorktreeList`), joined on the checkout's path.

### 3.8 The CLI

`src/main.rs`, `docs/cli.md` — `codegg launch`, with `--project`, `--fork`,
`--worktree`, `--base`, `--branch` and `--idempotency-key`. The old
`codegg --fork <id>` semantics are unchanged: a fork still copies history
into the parent's workspace, and a launch is a separate request.

### 3.9 The checkout may not be creatable on this host

`git worktree` is classified as requiring **contained** execution
(`egggit::process::classify_git_request` → `CleanSmudgeFilter`: a checkout
can run a smudge filter). This revision has **no contained Git backend**:
`GitEnvPolicy::apply` and `apply_sync` both turn a contained request into
`BackendUnavailable`, and `WorktreeService::create_reserved` — the only path
that creates a checkout — goes through the sync boundary.

So on this host the product's own checkout creation **always fails closed**,
and the plan anticipates exactly that: *"If the available Git backend cannot
safely create a managed checkout on a platform, the request must fail closed
and retain recoverable state."*

What that means, and what is proven:

- the launch refuses with the backend's own error, so the user learns
  *why*;
- **no session is created** — the failure happens before the session step;
- the launch's durable record moves to `needs_attention` with the
  `checkout_failed` category. The record is *terminal*: it is waiting for
  an operator, not for a resume, so it is found through the project's
  launch list rather than through the unfinished set;
- the **checkout record is left behind**, marked `orphaned` with a
  `git_error` health — the state the TUI's `⚠` marker reads. The
  directory was never created (the `git worktree add` never ran), and the
  record is **not** cleaned up, because it is the evidence of what
  happened.

`tests/session_launch_service.rs` proves the fail-closed path, and proves
the **placement** semantics against a **real** linked worktree that the
fixture creates with a plain `git worktree add` and records in the state
`create_reserved` would have set. The product's checkout *creation* on a
host with the contained backend is **not** claimed here.

## 4. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| A launch fails closed when the backend is unavailable | `tests/session_launch_service.rs::a_launch_fails_closed_when_the_managed_git_backend_is_unavailable` |
| The failure leaves no session and a visible record | the same test: the record is `needs_attention` / `checkout_failed` |
| The failed checkout is marked for an operator, and nothing is cleaned up | the same test: the worktree record is `orphaned` / `git_error`, and the directory does not exist |
| A session lands in a real linked checkout | `…::a_launch_places_a_session_in_an_existing_managed_checkout` |
| The checkout is a linked worktree, not a copy | the same test: the `.git` entry is a **file** |
| The session's canonical binding names the launch's project | the same test, plus `bind_checkout_workspace` |
| The checkout's workspace joins the launch's project | the same test: `workspace_binding.project_id` and `repository_id` |
| A fork moves into the checkout and leaves the parent alone | `…::a_fork_launch_moves_the_child_session_into_the_checkout` |
| A retry converges on one checkout and one session | `…::a_retry_with_the_same_key_converges_on_one_checkout_and_one_session` |
| Two simultaneous requests converge | `…::simultaneous_identical_requests_converge_on_one_checkout_and_one_session` |
| An unresolvable base ref creates nothing | `…::an_unresolvable_base_ref_fails_closed_and_creates_nothing` |
| A conflicting target branch is refused | `…::a_conflicting_target_branch_is_refused_and_creates_nothing` |
| Another project's checkout is refused | `…::a_launch_naming_a_checkout_of_another_project_is_refused` |
| A checkout another run holds is refused | `…::a_launch_naming_a_checkout_that_another_run_holds_is_refused` |
| A nonexistent checkout is refused | `…::a_launch_naming_a_checkout_that_does_not_exist_is_refused` |
| The store contract | `crates/codegg-core/tests/session_launch.rs` (11 tests) |
| The TUI shows the checkout, the root and the attention state | `src/tui/components/dialogs/session.rs::checkout_surface_tests` |
| The TUI's create/reuse/fork actions | the same module: `the_create_action_asks_for_a_new_checkout`, `the_reuse_action_names_the_selected_checkout`, `the_reuse_action_refuses_when_the_session_has_no_checkout`, `the_fork_action_names_the_parent_session` |
| The plain `f` fork is unchanged | the same module: `the_plain_fork_key_still_forks_into_the_same_workspace` |
| The fork's ancestry is shown by name | the same module: `a_fork_names_the_session_it_came_from` |

## 5. The named gap

Three of work package E's cases need a **fault injector** the codebase does
not have: a post-create SQLite failure, a cancellation between the Git
registration and the session bind, and a symlinked checkout path. They are
**not claimed**, and they are not simulated by a fake that would prove
nothing about the filesystem. The properties they cover are argued in
`architecture/session_launch.md` ("The narrow windows") instead, and the
*observable* halves of the same properties — a failed launch leaves a
visible `needs_attention` record, and a launch never cleans up — are tested
directly.

Two more are **not claimed** for the same reason:

- **The product's checkout creation on a host with the contained Git
  backend.** This revision has no contained backend, so the creation path
  always fails closed here (§3.9). The fail-closed behaviour is proven; a
  real creation is not.
- **Windows path-length handling.** The linked `.git`-file case is
  exercised on Linux.

## 6. Verification

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo test --test session_launch_service` | **10 passed** |
| `cargo test -p codegg-core --test session_launch` | **11 passed** |
| `cargo test -p codegg-core --lib` | **882 passed** |
| `cargo test -p codegg --lib checkout_surface_tests` | **10 passed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `scripts/verify.sh quick` | pass |
| `python3 scripts/check_tui_project_authority.py` | pass |
| `python3 scripts/check_daemon_listing_authority.py` | pass |
| `python3 scripts/check_tui_transport_authority.py` | pass |
| `python3 scripts/check_authorization_matrix.py` | pass |
| `python3 scripts/check_audit_coverage.py` | pass |
| `python3 scripts/check_scheduler_bypass.py` | pass |
| `python3 scripts/check_queue_write_transaction_shape.py` | pass |
| `python3 scripts/check_writer_disposition.py` | pass |
| `python3 scripts/check_project_catalog_invariants.py` | pass |
| `python3 scripts/check_config_merge_coverage.py` | pass |
| `bash scripts/check-core-boundary.sh` | pass |

### A clippy finding this milestone surfaced

The **newer** clippy on this host (1.99.0) has the
`assertions_on_constants` lint, and it fired on the two storage-layout
**floor** assertions that M006's own v85 bump had introduced in
`crates/codegg-core/tests/user_prompt_queue.rs` and
`…/user_prompt_queue_mutation.rs`: a comparison between two constants has
a value the compiler folds away, and a folded assertion is not a test.

Both now assert the floor against the **durable** layout — the version
read from `migration_version` — rather than against the constant. The
durable value is what the floor is actually about, and the equality
between the durable layout and the constant is still asserted directly
above in both files.

### What was **not** run

The **full workspace test suite** (`cargo nextest run --workspace --locked
--profile ci`) was **not** completed on this host: the run stalled for more
than twenty minutes on a cargo lock held by another session on the same
machine and was cancelled. It was never observed to fail — it never got past
the lock — but it was not observed to pass either. The same contention
delayed this milestone's own builds.

The pre-existing `check_policy_surface.py` failure (on `tool_batch.rs`) and
the three environmental `agent_loop_harness` failures (this machine's
`~/.cache` is mode 777 and `src/scratch.rs:474` refuses a group/world-writable
cache root) are unchanged by this milestone; pointing `XDG_CACHE_HOME` at a
700-mode temporary directory makes that suite pass 67/67.

## 7. Known gaps and deferred work

- **No shared checkout and no shared mutation.** M006 places a session in an
  *isolated* checkout. A checkout another run currently holds is refused,
  precisely because sharing it is not what this milestone enables.
- **The launch does not take the worktree lease.** The lease is what fences
  writers, and the writer that works in the checkout is the session's own
  agent run. A lease held by a synthetic owner would fence that run out
  (`acquire_lease` refuses a checkout that already has an owner) and nothing
  would ever release it.
- **The launch's branch and base ref default.** The TUI's actions use the
  service's defaults (the repository's HEAD, a minted
  `codegg/worktree/<id>`); the CLI names them explicitly. A prompt in the
  TUI was not added: a modal text prompt is a separate surface, and the
  defaults are the honest ones.
- **The launch surface is the session dialog.** The plan's wording ("existing
  project/session screens") is read as the session screen, and the three
  actions are keys on it rather than a new dialog.

## 8. What this milestone did **not** do

- It did not enable shared checkout execution or shared-write leases.
- It did not change what `codegg --fork` or the TUI's `f` fork mean.
- It did not make a transcript fork look like a worktree fork. A session is
  in a new checkout only when a launch put it there.
- It did not clean up a failed launch's artefacts, and it did not enable the
  M005 writer containment. Those remain blocked behind C003–C006.
