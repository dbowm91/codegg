# M005 Writer-Containment Corrective C004 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-workstream-m005-writer-containment-corrective/004-nonbroker-writer-sink-mediation.md`
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md` (C004)
Repository baseline reviewed: `1eeecb00` (post-merge-reconciliation of `work/agent-workstream-m005-writer-audit` with `origin/main` `a9f70375`)
Decision: `plans/adrs/ADR-0047-restricted-agent-shared-checkout-writer-contract.md` (**accepted 2026-10-19, option B**)
Predecessor closure: `plans/closure/agent-workstream-m005-writer-containment-corrective/003-status.md`

## 1. Executive finding

C003 put the writer fence in the **model's** writer path. C004 found the
other half: CodeGG has writers that never reach the broker, and every one
of them reached an active checkout unguarded.

The gap was not hypothetical and it was not small. The document service, the
change-review LSP applier, project initialization, checkpoint restore, the
managed-worktree admin requests and the HTTP file routes all wrote into a
checkout with no writer authority at all. A human with a broader approval
profile could write into a checkout that a live agent-run group was
reserving, and nothing in the system noticed. The model's own writers were
serialized; the human's were not.

C004 closes that with an **authoritative pre-effect reservation check**:

- `crates/codegg-core/src/checkout_reservation.rs` is the authority. It
  answers one question from durable state only: *is this checkout reserved
  by a live agent-run group, and who may write it?* The answer is derived
  from the managed worktree record (the canonical checkout identity), the
  lease's owner run, and the durable run-group record. Nothing is asserted
  by a caller.
- `src/core/checkout_mediation.rs` is the daemon adapter. It names every
  mediated sink, owns the typed conflict a refused caller receives, and
  owns the live durable deny event.
- `src/core/daemon.rs` runs the gate **after** the authorization decision
  and **before** dispatch. A refused writer therefore has no side effect at
  all: no file write, no document revision, no index, branch or metadata
  change, and no accepted launch record.

The rule mirrors the fence's own rule rather than inventing a second one.
A checkout is *reserved* when its lease owner belongs to a **live**
(non-terminal) agent-run group. The group's members may write it; nobody
else may — not the project owner with a broader approval profile, and not a
principal from another project.

**C004 is still not shared-write readiness, and it did not become one.** No
production path places two runs on one checkout: the work-order
`shared`/`serialized` placement is still refused at the coordinator, and the
scheduler still gives every delegated run its own isolated worktree. The
qualification constructs the shared case directly, which is the point: the
mediation is proved before any product path is allowed to depend on it.

### What is live in production today, and what is not

**Live: the mediation gate.** Every `CoreDaemon` owns a
`CheckoutMediator` over the durable reservation authority. Every
checkout-writing request and HTTP route listed in §3 asks it before its
first effect.

**Not live: shared mutation.** The gate refuses *more* than the shared
profile needs, never less. A reserved checkout is refused to every writer
outside the group, and the group itself is still not reachable.

**Not live: the writer permit for these sinks.** The mediation is a
*check*, not a permit. It takes no lock and holds no writer permit, so it
cannot invert the order between the document revision lease, the repository
lock and the C002 permit. That is the deliberate narrow choice the C004
plan offers for rare admin, restore and project-init paths, and it is the
choice that keeps the gate deadlock-free.

## 2. The mechanism

### 2.1 The authority (`crates/codegg-core/src/checkout_reservation.rs`)

`CheckoutReservationService` resolves a target path to a **canonical
checkout identity** and then to a reservation:

1. **Canonical identity.** The target path is made absolute, lexically
   normalized (`.` and `..` folded), and canonicalized over its longest
   existing prefix. A path naming a file that does not exist yet resolves
   to the identity of its existing parent, so a write target and the
   checkout that contains it agree before the write lands. A symlinked
   parent collapses to its real path, so an alias resolves to the same
   checkout. A *linked worktree* stays distinct from its parent repository:
   they are different directories and therefore different checkouts.
2. **Containment.** The managed worktree whose canonical root *contains*
   the target is the checkout. The deepest containing root wins, so a
   nested managed checkout resolves to itself. A path outside every managed
   checkout is not a managed checkout at all.
3. **Reservation.** The worktree record names the lease's owner run. If
   that owner belongs to a **live** agent-run group, the checkout is
   reserved, and the claim carries the group, the owner and the group's
   members.

`CheckoutReservation::authorizes(run)` is the actor rule: the lease owner
and every member of the live group may write; a writer with no run (a human
or an administrative path) may not.

The service is read-only. It never acquires a permit, never waits, and never
mutates. A lookup failure is **not** treated as "unreserved": the daemon
refuses the write when the authority cannot be consulted.

### 2.2 The daemon adapter (`src/core/checkout_mediation.rs`)

- `CheckoutWriterSink` names all fourteen mediated sinks.
- `CheckoutMediator` answers the reservation question for a workspace root
  or a managed worktree id. It is disabled (`None`) only for a daemon with
  no durable worktree or group state, which can never share a checkout.
- `CheckoutConflict` is the typed refusal: the sink, the checkout's durable
  identity, the live group, the lease owner, and a recovery path
  ("retry after that group finishes, or write to a separate checkout"). It
  never names a path, a file body or another run's content.
- `emit_checkout_writer_refusal` is the live owner of the durable deny
  event.

### 2.3 The gate (`src/core/daemon.rs`)

`mediate_checkout_write` runs after the M003 authorization decision and
before dispatch. It resolves the target from durable identities — the
workspace the request names, or the workspace that owns the document — never
from a path in the payload, and never from the process CWD. On refusal it
emits the durable deny event and answers with
`CoreResponse::Error { code: "checkout_reserved", .. }`.

`mediate_http_checkout_write` is the HTTP-surface entry point. It reaches
the same authority and the same event.

### 2.4 The lock order

The mediation takes no lock, so it cannot invert any order. The concrete
orders it was placed inside:

| Sink | Order at the sink | Mediation position |
|---|---|---|
| Document save | document operation lock → document revision lease → repository lock → write | after the lease, **before** the repository lock and the write; `capture_save` is read-only, so a refusal leaves the revision and the dirty-file state untouched |
| Document create/delete | repository lock → write | **before** the repository lock. The create path's *own* refresh used to re-acquire the repository lock the caller still held — a self-deadlock, now fixed (§6.5) |
| Project-init publish | atomic temp → rename | **before** the temp file is written |
| LSP preview apply | checked digest → write | **before** the digest check, so a refused apply never touches the file |
| Checkpoint restore | repository lock → restore | **before** the repository lock |
| Managed worktree cleanup/archive | worktree lease → remove | **before** the removal |
| HTTP file write/delete | authorize → `create_dir_all` → write | **before** the first filesystem mutation |

## 3. The sink inventory

Fourteen sinks are mediated. Every one of them is a **final effect owner**,
not a frontend button.

| # | Sink | Final effect owner | Class | Mediation site | Refusal evidence |
|---|---|---|---|---|---|
| 1 | `CoreRequest::DocumentCreate` | `src/lsp/mutation.rs::checked_workspace_file_create` | human (TUI) | `src/core/daemon.rs` | `a_document_create_into_a_reserved_checkout_is_refused_with_no_effect` |
| 2 | `CoreRequest::DocumentSave` | `src/lsp/mutation.rs::checked_workspace_text_write` | human (TUI editor) | `src/core/daemon.rs` | `an_editor_save_into_a_reserved_checkout_is_refused_with_no_effect` |
| 3 | `CoreRequest::DocumentDelete` | `src/lsp/mutation.rs::checked_workspace_file_delete` | human (TUI) | `src/core/daemon.rs` | the same gate arm as create and save; the guard's marker check covers the arm |
| 4 | `CoreRequest::LspPreviewApply` | `src/lsp/mutation.rs::checked_workspace_text_write` | human (change review) | `src/core/daemon.rs` | `an_lsp_preview_apply_into_a_reserved_checkout_is_refused_with_no_effect` |
| 5 | `CoreRequest::ProjectInitPublish` | `src/core/daemon_project_init.rs::publish_checked` | human (project init) | `src/core/daemon.rs` | `a_project_init_publish_into_a_reserved_checkout_is_refused_with_no_effect` |
| 6 | `CoreRequest::EditCheckpointUndo` | `crates/codegg-core/src/snapshot/checked_restore.rs` | human (editor undo) | `src/core/daemon.rs` | covered by the gate's EditCheckpoint arms |
| 7 | `CoreRequest::EditCheckpointUndoLatest` | same | human | `src/core/daemon.rs` | same |
| 8 | `CoreRequest::EditCheckpointReapply` | same | human | `src/core/daemon.rs` | same |
| 9 | `CoreRequest::EditCheckpointReapplyLatest` | same | human | `src/core/daemon.rs` | same |
| 10 | `CoreRequest::ManagedWorktreeCleanup` | `crates/codegg-core/src/worktree_service.rs::cleanup` | admin | `src/core/daemon.rs` | `a_managed_worktree_cleanup_of_a_reserved_checkout_is_refused` |
| 11 | `CoreRequest::ManagedWorktreeArchive` | `crates/codegg-core/src/worktree_service.rs::archive` | admin | `src/core/daemon.rs` | same arms |
| 12 | HTTP `POST /file` | `src/server/routes/file.rs::write_file` | human (HTTP) | `src/server/routes/file.rs` | `an_http_write_into_a_reserved_checkout_is_refused_with_no_effect` |
| 13 | HTTP `DELETE /file` | `src/server/routes/file.rs::delete_file` | human (HTTP) | `src/server/routes/file.rs` | `an_http_delete_of_a_reserved_checkout_is_refused_with_no_effect` |
| 14 | HTTP `POST /workspace` | `src/server/routes/workspace.rs::create_workspace` | human (HTTP) | `src/server/routes/workspace.rs` | `an_http_workspace_create_into_a_reserved_checkout_is_refused` |

The **model's** LSP apply is a different route: `tool.lsp_preview_apply`
reaches the same applier through the broker, and C003's fence already takes
the permit there. The daemon request above is the *human* change-review
path, which had no fence at all. The two are now separate: the broker path
serializes through the permit, the human path is refused.

### 3.1 Surfaces that are explicitly not mediated

| Surface | Why not | Evidence |
|---|---|---|
| `src/server/routes/project.rs` | Its only filesystem effect is creating the **project root**, and only when the requested absolute path does not exist yet. It cannot write into an existing, let alone reserved, checkout. Every other project mutation lands in the user-scoped catalog. | `module.server_project_route` → `new-root-only` |
| `src/ide/mod.rs` | Writes only into a temporary diff directory, never the checkout. | `module.ide` → `out-of-contract` |
| The project-local `.codegg/` subtree | CodeGG-managed sidecar state. Classified individually in §5. | the manifest's stable store policy |
| The memory and habit stores | The roots are `dirs::config_dir()/codegg/memory` — the OS user configuration directory. No constructor takes a workspace or project path. **The manifest previously classified these as checkout writers; that was wrong and is corrected.** | `module.memory`, `module.memory_habit` → `out-of-contract` |

## 4. The durable deny event

C003's closure recorded, as accepted finding 1, that the deny side of the
restricted writer profile had no durable *event* row: a refusal that never
reached the permit store left nothing behind. C004 adds it.

- **`AuditAction::CheckoutWriterRefusal`** (wire `checkout_writer_refusal`) —
  a new action in `codegg-core`'s event-coverage matrix. The action is
  `live_mapped: true`, and it has a coverage row naming the owner
  (`daemon:checkout_mediation`), the actor (`transport-bound principal`),
  the scope (`via_session`) and the allowed metadata keys.
- **`DAEMON_LIVE_AUDIT_HOOKS`** — a new table in
  `crates/codegg-core/src/audit_instrumentation.rs`. It is the sibling of
  the M004 executor-hook table: a component that owns a live audit action
  *outside* the daemon operation mapping. The refused operation never ran,
  so the refusal has no operation mapping; the gate owns the evidence.
- **`DAEMON_HOOK_OWNER_PINS`** — the declarative owner table in
  `src/core/checkout_mediation.rs`. `scripts/check_audit_coverage.py` parses
  it and fails closed if the owner file moves or the emit symbol disappears.
  The marker check looks for the emit symbol's **definition**
  (`fn emit_checkout_writer_refusal(`), not merely the string, because the
  pin table itself names the symbol.
- **The event's content.** The sink, the operation, the group id, the
  decision outcome, the session/turn/project locators and the worktree id.
  No path, no file body, no other run's content. The refusal is attributed
  to the transport-bound principal and carries the M003 decision provenance.

The coverage guard grew one check
(`check_daemon_hook_table_is_authoritative`) and one exemption in the
live-mapped rule. Its non-vacuity was proved on the real tree: renaming the
emit function makes the guard fail with
`src/core/checkout_mediation.rs missing hook marker emit_checkout_writer_refusal`.

## 5. The project-local state disposition (the stable store policy)

The C004 plan's work package D asked for an explicit disposition for the
project-local state under `<checkout>/.codegg/`, and refused a false "outside
checkout" claim. The inventory behind it produced two findings that
contradicted the plan's premises, and both are recorded rather than smoothed
over.

**The stable store policy.** CodeGG's project-local state lives under
`<checkout>/.codegg/`. It is physically **inside** the checkout and is
never claimed to be outside it. It is CodeGG-managed sidecar state: no path
under it is tracked by Git, and writing it changes no repository file, no
Git index entry, no branch and no ref. The checkout mediation governs
checkout **content** — the files Git tracks, plus the Git index, branches
and refs — and every `.codegg/` writer root is classified individually in
`docs/writer-disposition.toml`:

| Classification | Meaning | Roots |
|---|---|---|
| `namespaced-state` | the path carries a per-actor identifier, so two actors cannot write the same file | `module.goal_checkpoint` (goal id), `module.research_store` (run id), `module.test_runner_runner` (timestamp + uuid), `module.python_script_source_store` (content digest), the run bodies under `module.run_store` |
| `shared-sidecar` | the path is a fixed name every actor writes | `module.run_store` (the `index.jsonl`), `module.workspace_services` (the `.codegg` directory), `module.skills_publish`, `module.skills_promotion`, `module.test_runner_index` |
| `compatibility-store` | the legacy project-local SQLite file | `module.storage` |

For a `shared-sidecar` root the accepted consequence is recorded in the
root's note: **two concurrent CodeGG writers may lose one line in CodeGG's
own sidecar index. Nothing in the repository is affected.** That
consequence is disclosed, not silently accepted, and §10 carries it as an
open item.

### 5.1 The legacy project-local database is still written in production

The C004 plan stated that the legacy `<project>/.codegg/sessions.db` is
"retained only for backward-compatible import". **That premise is false.**
`storage::init_migrated_legacy_project_store` creates the directory,
creates the file and runs the full schema migration against it, and six
production entry points call it:

| Call site | Entry point | Actor |
|---|---|---|
| `src/main.rs:1819` | `codegg sessions` | human CLI |
| `src/main.rs:1956` | `codegg session-view` | human CLI |
| `src/main.rs:2018` | `codegg export` | human CLI |
| `src/main.rs:2060` | `codegg import` | human CLI (also inserts sessions and messages) |
| `src/main.rs:3307` | TUI, non-socket branch | human TUI |
| `src/main.rs:4376` | `codegg server --standalone-core` | admin |

The disposition is `compatibility-store`, with the facts stated plainly:
the **live** store is user-scoped (`~/.local/share/codegg/codegg.db`, see
`crates/codegg-core/src/storage/paths.rs`), this file is a backward-
compatible artifact addressed by the **process CWD**, it is physically
inside a checkout when CodeGG is launched from one, and it never writes
repository content, a Git index, a branch or a ref. The importer
(`codegg_core::migration::migrate_legacy_project_database`) is test-only.

Those six call sites run **outside the daemon**, so the mediation cannot
reach them: they have no daemon, no run identity and no reservation
authority. Making the legacy store read-only would change the compatibility
contract of four CLI commands and is not a mediation decision; it is
recorded in §10 as a follow-up.

## 6. Defects found and fixed

1. **The `commit` tool was an unmediated checkout writer.** C003 routed the
   typed Git mutations through the fence, but `src/tool/commit.rs` never
   called `acquire_writer_permit` at all — it staged paths and moved a
   branch with no writer authority, while `src/tool/git.rs` did take the
   permit for the same repository. C004 found it by inventory, not by
   reading the C003 diff. The permit is now taken after the input
   validation and before the first staging, and is held through the commit.
   It is settled on every exit path: a failure that may have staged reports
   `Unproven`, so the checkout is **parked** rather than handed to the next
   writer with a dirty index. The effect was extracted into
   `commit_effects` so the permit is held *around* the whole effect rather
   than around a fragment of it.
2. **Six project-local writer roots were outside the guard's sweep.**
   `src/research`, `src/python_script`, `src/skills` and `src/test_runner`
   were not scanned, so their writers were undeclared. The sweep now covers
   them and the manifest declares the six roots they contain.
3. **The guard's test-module stripper could silently give up.**
   `strip_test_modules` brace-matched over raw source, so an unbalanced
   brace inside a string literal (`"{"` in a YAML fixture) desynchronised
   the matcher; the matcher then failed to find the end of the test module
   and returned the **whole file** unstripped. The result was a test-only
   module reported as a production writer root. The stripper now blanks
   string literals and comments before matching, and the guard's self-test
   covers the case.
4. **The memory roots were misclassified.** `module.memory` and
   `module.memory_habit` were `checkout-writer`; their roots are
   `dirs::config_dir()/codegg/memory`. Corrected to `non-checkout` /
   `out-of-contract`.
5. **`CoreRequest::DocumentCreate` self-deadlocked on the repository lock.**
   The create path holds the workspace's repository lock across
   create-then-open, deliberately, so a concurrent write to the same new
   path cannot land between the two steps and be silently adopted as the
   snapshot. It then called `refresh_external_disk`, which acquires the
   *same* lock again — a non-reentrant `tokio::sync::Mutex` owned by the
   same workspace-services bundle, so the second acquisition waited for
   the guard the caller was still holding. The C004 qualification found
   it: the refusal tests never reach the handler, and the first test that
   *did* reach it hung. The refresh now takes the lock only when the
   caller does not already hold it. This was latent rather than
   user-visible — no TUI path sends the workspace-scoped
   `CoreRequest::DocumentCreate` — but it is a real deadlock on a path the
   protocol exposes, and the acceptance matrix names it.
6. **The HTTP surface had no conflict shape.** The C004 refusal needs a
   typed conflict, and the server's error envelope had no 409. The
   existing `StorageError::Conflict` — whose doc comment already describes
   exactly this case ("the write did not fail: it was *refused*") — is now
   mapped to **409 Conflict**, and its message is carried in the response
   body so the caller can see the recovery path. No new `AppError` variant
   was added.

## 7. The manifest and the guard

`docs/writer-disposition.toml` grew the C004 vocabulary
(`mediated-gate`, `namespaced-state`, `shared-sidecar`,
`compatibility-store`, `new-root-only`) and a `mediation` field
(`writer-fence`, `checkout-gate`, `none`). Every root declares how it is
mediated; a mediated root names the file that holds its mediation call and
the markers that identify it.

`scripts/check_writer_disposition.py` grew four checks:

- every root declares a `mediation`, and the value is in the vocabulary;
- a `mediated-gate` root names a `mediation_site` and `mediation_markers`,
  the site file exists, and every marker is present in it;
- `mediated-gate` ⟺ `checkout-gate` and `fenced-native` ⟺ `writer-fence`;
- **every sink the mediator declares is claimed by exactly one manifest
  root** — a sink that quietly lost its manifest row would otherwise be
  mediated in code but undeclared, and nothing would fail.

Non-vacuity was proved on the real tree, not only in the self-test:

| Injected violation | Guard result |
|---|---|
| A mediated sink loses its manifest claim | `mediated sink 'CheckoutWriterSink::DocumentCreate' is declared by src/core/checkout_mediation.rs but no manifest root claims it` — exit 1 |
| The source loses the mediation call | `entry 'module.document_service' lost its mediation: 'CheckoutWriterSink::DocumentDelete' is no longer present in 'src/core/daemon.rs'` — exit 1 |
| A mediated root names a missing site | `entry 'module.server_file_route' names a missing mediation site 'src/server/routes/gone.rs'` — exit 1 |
| The daemon gate loses its emit definition | `src/core/checkout_mediation.rs missing hook marker emit_checkout_writer_refusal (action checkout_writer_refusal)` — exit 1 |

The fixture suite is the primary instrument: it drives each mediated sink
through a **real `CoreDaemon`**, a **real HTTP route** and a **temporary Git
checkout**, and fails when a sink loses its mediation.

## 8. The acceptance matrix

| Acceptance item | Result | Evidence |
|---|---|---|
| Two mutation sources alternate; the non-broker source receives a pre-effect refusal; no file/index/branch/metadata change on denial | **met** | the daemon suite asserts the target file does not exist after each refusal; the HTTP suite asserts the same and that the response is 409 |
| Other principals cannot acquire another group's permit; the own-project human cannot bypass a reserved checkout with a broader profile | **met** | the human path carries no run, so `authorizes(None)` is false for a reserved checkout. The daemon path's principal is the local owner — the broadest profile in the system — and it is still refused |
| No lock-order inversion or deadlock between the pending document lock and the writer permit; cancellation/restart preserves revision and dirty-file state | **met** | the mediation takes no lock and holds no permit; it is placed before the repository lock and before the write. `capture_save` is read-only, so a refusal advances no revision |
| `.codegg/runs` and the legacy DB have an explicit verified disposition and a test; no false "outside checkout" claim | **met** | §5 and the manifest's stable store policy. The legacy DB's *production write path* was corrected against the plan's premise |
| Separate checkouts continue ordinary non-blocking API writes; read-only file tree and temp diff need no writer authority | **met** | `a_document_create_into_an_unreserved_checkout_succeeds` and `an_http_write_into_an_unreserved_checkout_succeeds`; the read-only surfaces were never mediated |
| Genuine integration tests through a real `CoreDaemon`, HTTP route and temporary Git checkout | **met** | `tests/checkout_reservation_c004.rs` (daemon) and `tests/checkout_reservation_c004_http.rs` (HTTP), over `tests/common/checkout_reservation.rs` |
| Run focused suites, fmt, quick, Clippy and the applicable ownership checks | **met** | §9 |

## 9. Verification evidence

Exact commands, run on the merged-and-modified tree at the commit this
record accompanies.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo check --workspace --all-targets` | clean |
| `scripts/verify.sh quick` | exit 0 (`Quick verification passed.`) — this includes the core-boundary guard, the audit-coverage guard, the writer-disposition guard, the agent-schema check, the config-merge coverage check and `cargo check --workspace --all-targets --locked` |
| `scripts/check_writer_disposition.py` | `writer-disposition check ok: 55 declared roots, 17 writer tools, 36 writer modules` |
| `scripts/check_writer_disposition.py --self-test` | `writer-disposition self-test ok` |
| `scripts/check_audit_coverage.py` | `All audit coverage invariants verified.` (11/11 checks) |
| `scripts/check_audit_coverage.py --self-test` | `audit-coverage self-test ok` |
| `cargo test -p codegg --test checkout_reservation_c004` | 14 passed, 0 failed (9 C004 tests plus the 5 shared `tests/common` helpers the binary also runs) |
| `cargo test -p codegg --features server --test checkout_reservation_c004_http` | 10 passed, 0 failed (5 C004 tests plus the 5 shared `tests/common` helpers the binary also runs) |
| `cargo test -p codegg --test writer_fence_c003` | 10 passed, 0 failed |
| `cargo test -p codegg --test writer_permit_service` | 12 passed, 0 failed |
| `cargo test -p codegg --test tool_broker_integration` | 25 passed, 0 failed |
| `cargo test -p codegg --test writer_disposition_guards` | 5 passed, 0 failed |
| `cargo test -p codegg --lib tool::` | 711 passed, 0 failed (the `commit` tool's new fence) |
| the four C003 suites together, after the `commit`-tool fence and the document-create deadlock fix | 52 passed, 0 failed |
| `cargo test -p codegg-core --test writer_permit` | 21 passed, 0 failed |

### 9.1 What was NOT run, and why

- **No hosted CI run.** The branch is local-only and was not pushed.
- **No full-workspace `nextest`/`cargo test` sweep.** The host is shared
  with other sessions' cargo builds; the suite could not complete here.
  The scoped suites above, workspace Clippy and `verify.sh quick` were the
  evidence.
- **The Eggsearch pair qualification was not re-run.** It needs a pinned
  Eggsearch checkout and a network-free build.
- **`scripts/check_policy_surface.py` still fails on `tool_batch.rs`.**
  Identical on the base branch, not part of `verify.sh quick` or CI, and
  untouched by C004.

## 10. Known limits and non-claims

**Non-claims.**

- C004 does **not** enable shared mutation. No production placement puts two
  runs on one checkout; the work-order `shared`/`serialized` policy is
  still refused and the scheduler still isolates every delegated run.
- C004 does **not** make the non-broker sinks take the C002 writer permit.
  It is a pre-effect check, deliberately, so it cannot invert the
  document/repository/permit lock order. A sink that wants the *permit*
  semantics — a writer that should wait rather than be refused — is a
  C006 question.
- C004 does **not** mediate CodeGG's project-local sidecar state. It gives
  it an explicit, evidenced disposition instead (§5).
- The qualification constructs the shared case directly. Nothing here is a
  claim about a production shared checkout.

**Open items, none blocking C005.**

| # | Severity | Item |
|---|---|---|
| 1 | medium | The run-store index `<workspace>/.codegg/runs/index.jsonl` is a fixed name every actor appends to. The only serialization is an **in-process** mutex held per `FsRunStore` *instance*, and there are at least two instances over the same root (`crates/codegg-core/src/workspace_services.rs` and `src/tool/factory.rs`), so their mutexes do not exclude each other even in one process. `rewrite_index_locked` does load-index → write-tmp → rename, and a concurrent append landing between the load and the rename is silently lost. The same shape exists at `<workdir>/.codegg/test-runs/index.json`. This is CodeGG's own sidecar index — no repository content, index, branch or ref is affected — but it is a real data-loss window and it deserves a cross-process lock. |
| 2 | medium | The goal checkpoint path is built from the **typed project id** (`src/core/daemon_goals.rs`: `PathBuf::from(&project_id)`), so it resolves relative to the **daemon's process CWD** rather than the session's workspace root. That is the exact path-resolution class AGENTS.md warns about, and it means the goal checkpoint is usually *not* inside the checkout at all. C004 records it rather than moving it: changing where CodeGG stores its own state is a data-location decision, not a mediation decision, and the plan forbids moving user data on a presumed classification. |
| 3 | low | The legacy project-local store is still opened read-write by six production CLI/TUI entry points (§5.1). Those paths run outside the daemon, so no mediation can reach them. Making the legacy store read-only — or rehoming it — would change the compatibility contract of four CLI commands. |
| 4 | low | The mediation is a *check*, not a *permit*. A reserved checkout is refused to every outside writer even when the group's own writer is idle, so an ordinary write can be refused while the checkout is technically writable. The narrow approach was chosen deliberately; the trade is availability for deadlock-freedom. |
| 5 | low | The mediation's answer is a point-in-time read of durable state. A group that starts *after* the check and before the write is not observed by that write. The broker path does not have this window (it holds a permit across the effect); the non-broker path does. |

## 11. C005 unblocking

C005 owns the **process** denial contract: the eleven CodeGG-managed process
launchers are declared `denied-process` and the guard scans them, but no
production path refuses a process-backed writer yet. C005 makes the
declaration a behaviour.

C005 is **not blocked by C004**. C004's own preconditions are met: the
non-broker sinks are mediated or classified, the durable deny event exists,
and the manifest's remaining `fenced-native` roots that are not native tool
routes have been re-dispositioned (§3, §5). C005 may start on C004's
closure.

C006 remains the milestone that runs the full barrier matrix against a
shared checkout in production, and it inherits §10 items 1, 2 and 5 as
known limits it must either close or re-record.

## 12. Historical records preserved

This record does not rewrite any earlier closure. `001-status.md`,
`002-status.md`, `003-status.md` and the merge-reconciliation record
`004-merge-reconciliation-status.md` are unchanged historical evidence.
C004's own record is this file.
