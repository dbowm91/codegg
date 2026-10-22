# C001 Evidence — Non-Broker Project-Root Writer Sinks

Baseline SHA: 38f74b4632ea9a780df73ad8a96e0943fd492db2
Branch: work/agent-workstream-m005-writer-audit
Scope: CodeGG-controlled writers that mutate a checkout **without** going through the model ToolBroker.
Method: read-only source audit at the baseline SHA. Every `file:line` below was read directly in this
worktree; no line range is cited that was not opened. Where a fact could not be established from
source, it is marked `UNPROVEN:`.

Disposition vocabulary used (from the C001 contract): `read-proven`, `fenced-native-candidate`,
`denied-process`, `fenced-or-denied`, `unsupported`, `private-scratch`, `out-of-contract`.

## Sink: HTTP file write

- Final effect owner: the axum handler itself — `tokio::fs::write(&full, &req.content)` at
  `src/server/routes/file.rs:238`. This is the physical write; there is no shared writer service
  underneath it.
- Checkout locator: `resolve_context` (`src/server/scope.rs:65`) resolves
  `(project_id, workspace_id)` or a unique `directory` into a `ProjectContext` whose
  `workspace_root` is the checkout root. `sanitize_path_from_root`
  (`src/server/routes/file.rs:27`) canonicalizes that root (`:30`), runs
  `check_path_for_symlinks` on the joined path (`:34`), canonicalizes the target or walks the
  components (`:41`-`:68`), and requires `resolved.starts_with(&root_canonicalized)` (`:70`).
  Identity is therefore **path-canonical**, not git-lineage: a path alias (symlink to the same
  directory) collapses to the same canonical root, but a **linked worktree is a distinct
  directory** and therefore a distinct root. No git common-dir / lineage resolution is consulted
  on this path — `lineage_key` exists only on the project catalog's `repository` row
  (`crates/codegg-core/src/project_catalog.rs:456`, `:764`-`:765`) and is not read by any writer.
- Authority path: route disposition `SharedAuthz` / operation `file_modify` / capability
  `file.modify` (`src/server/authz.rs:259`-`:265`), registered at `src/server/http.rs:304` behind
  `auth_middleware` (`src/server/http.rs:327`-`:330`, `src/server/middleware/auth.rs:54`).
  The handler authorizes **before** any filesystem mutation
  (`src/server/routes/file.rs:224`-`:231`) via `authz::authorize_project(... Capability::FileModify,
  "file_modify")` (`src/server/authz.rs:422`). Principal comes from the bearer token: a personal
  token binds a distinct principal, the legacy global bearer binds `LocalOwner`
  (`src/server/middleware/auth.rs:100`-`:114`, `crates/codegg-core/src/transport_auth.rs:52`-`:56`),
  and the auth-disabled env escape hatch also binds `LocalOwner`
  (`src/server/middleware/auth.rs:62`-`:67`).
  **Yes — an authenticated human bypasses concurrency protection here**, because there is no
  concurrency protection on this path at all. Human authorization is a *capability* decision, not
  a *concurrency* decision.
- Lifetime: request-scoped. The write completes inside the handler; nothing is deferred, queued, or
  persisted.
- Reachable callers: HTTP route only (`src/server/http.rs:304`). No TUI, desktop/ACP, or
  daemon-internal caller reaches this handler.
- Existing coordination (and its limits): project-level capability authorization only
  (`src/server/routes/file.rs:224`-`:231`) plus path containment
  (`src/server/routes/file.rs:27`-`:77`). It does **not** cover: the workspace repository lock
  (`WorkspaceLockTable`), any document revision lease, any base-disk-digest check, atomic
  replace, an edit checkpoint, a `FileChanged` event, or a run/turn id. `tokio::fs::write` is a
  **truncating in-place write**, not an atomic rename — a reader can observe a partially written
  file, and a crash mid-write leaves a truncated file. It also does **not** cover other writer
  classes (agent/broker writers, LSP preview apply, project init) and does **not** cover
  cross-process or cross-workspace contention.
- Adversarial bypass example: a TUI editor save of the same file and an HTTP write of the same
  file overlap. The editor save path performs a hash check against the base disk digest
  (`src/lsp/mutation.rs:223`-`:229`) and then atomically renames
  (`src/lsp/mutation.rs:243`). The HTTP write truncates in place
  (`src/server/routes/file.rs:238`). If the HTTP write lands between the editor's digest check and
  its rename, the editor's rename silently discards the HTTP write (lost update). If the HTTP
  write lands after the editor's rename, it clobbers the saved content and the editor never
  learns about it, because the HTTP path publishes no `FileChanged` event (contrast
  `src/core/daemon_documents.rs:768`-`:774`). Neither side is told a conflict occurred.
- Preliminary disposition: `fenced-or-denied`. The sink has a native fence available in-tree
  (`checked_workspace_text_write` + `WorkspaceLockTable`, both already used by the daemon document
  path), but today uses none of it. If the corrective cannot route this route through the shared
  final writer service, the route must be denied rather than left as an unfenced writer.
- Evidence: `src/server/routes/file.rs:210`, `src/server/routes/file.rs:222`-`:231`,
  `src/server/routes/file.rs:232`, `src/server/routes/file.rs:238`, `src/server/routes/file.rs:27`,
  `src/server/routes/file.rs:30`, `src/server/routes/file.rs:34`, `src/server/routes/file.rs:70`,
  `src/server/http.rs:304`, `src/server/http.rs:327`, `src/server/authz.rs:259`,
  `src/server/authz.rs:422`, `src/server/scope.rs:65`, `src/server/middleware/auth.rs:54`,
  `src/server/middleware/auth.rs:100`, `crates/codegg-core/src/transport_auth.rs:52`,
  `crates/codegg-core/src/project_catalog.rs:456`, `crates/codegg-core/src/project_catalog.rs:764`

## Sink: HTTP file delete

- Final effect owner: the axum handler itself — `tokio::fs::remove_file(&full)` at
  `src/server/routes/file.rs:276`.
- Checkout locator: identical to HTTP file write — `resolve_context`
  (`src/server/scope.rs:65`) then `sanitize_path_from_root`
  (`src/server/routes/file.rs:27`, `:30`, `:34`, `:70`). Path-canonical identity only; a linked
  worktree is a different root, a path alias is the same root.
- Authority path: `authz::authorize_project(... Capability::FileModify, "file_modify")` at
  `src/server/routes/file.rs:267`-`:274`; route disposition at `src/server/authz.rs:266`-`:272`;
  route registered at `src/server/http.rs:305` behind `auth_middleware`. No revision lease, no
  "is this file open in an editor?" check (contrast the daemon document delete, which does have
  that check at `src/core/daemon_documents.rs:537`-`:546`).
- Lifetime: request-scoped.
- Reachable callers: HTTP route only (`src/server/http.rs:305`).
- Existing coordination (and its limits): capability authorization + path containment. Does
  **not** cover: the repository lock, an edit checkpoint (so the delete is not undoable through
  the checkpoint mechanism), a `FileChanged` event, an "attached document" refusal, or a run/turn
  id. It does **not** cover other writer classes and does **not** cover cross-process contention.
- Adversarial bypass example: the TUI editor has the file open and holds a writer lease, and a
  concurrent `DELETE /api/file/delete` removes it. The editor's next save then writes to a path
  that no longer exists — `checked_workspace_text_write` requires the target to be an existing
  regular file and returns an error (`src/lsp/mutation.rs:211`-`:214`), so the save fails with
  `document_save_failed` rather than recreating the file. The user loses the edit with no
  "file was deleted underneath you" signal, because the HTTP delete published no event.
- Preliminary disposition: `fenced-or-denied` — same reasoning as HTTP file write.
- Evidence: `src/server/routes/file.rs:255`, `src/server/routes/file.rs:267`-`:274`,
  `src/server/routes/file.rs:275`, `src/server/routes/file.rs:276`, `src/server/http.rs:305`,
  `src/server/authz.rs:266`, `src/server/scope.rs:65`, `src/core/daemon_documents.rs:537`,
  `src/lsp/mutation.rs:211`

## Sink: daemon document save (human-originated editor save)

- Final effect owner: `crate::lsp::mutation::checked_workspace_text_write`
  (`src/core/daemon_documents.rs:739`) — the shared one-file checked write. Its physical effect is
  a temp file + `std::fs::rename` (`src/lsp/mutation.rs:234`-`:243`), i.e. an atomic replace, and
  it re-verifies the on-disk sha256 against the caller's expected hash before writing
  (`src/lsp/mutation.rs:223`-`:229`).
- Checkout locator: `self.workspaces.resolve(&workspace_id)` (`src/core/daemon_documents.rs:723`)
  yields `workspace.canonical_root`, which was canonicalized at registration
  (`crates/codegg-core/src/workspace.rs:484`, `:615`, `:639`). The write re-canonicalizes the root
  itself (`src/lsp/mutation.rs:207`) and validates the relative path against it
  (`src/lsp/mutation.rs:208`-`:214`). Path-canonical identity only — a linked worktree is a
  distinct root, and **no git common-dir resolution maps a linked worktree to its main
  checkout**, so the same repository reached through two worktrees is two independent roots,
  two independent locks, and two independent writer leases.
- Authority path: three layers.
  (1) Daemon-wide request authorization: `authorize_request`
  (`src/core/daemon.rs:805`, invoked at `src/core/daemon.rs:3812`) maps
  `CoreRequest::DocumentSave` to operation `document_modify` with
  `Capability::FileModify` (`crates/codegg-core/src/authorization/policy.rs:128`-`:133`).
  (2) Per-document operation lock: `operation_lock(&document_id, &project_id, client_id,
  &writer_lease)` (`src/core/daemon_documents.rs:688`; implementation
  `src/document_service.rs:552`) rejects a request whose writer lease does not match
  (`src/document_service.rs:568`-`:574`).
  (3) Revision + disk digest: `capture_save` (`src/core/daemon_documents.rs:697`;
  implementation `src/document_service.rs:618`) re-checks the lease
  (`src/document_service.rs:635`-`:641`) and the buffer revision
  (`src/document_service.rs:642`-`:644`), and returns the base disk digest
  (`src/document_service.rs:653`) that `checked_workspace_text_write` then re-verifies against
  the live file.
  Scope is taken from the attachment, never from the request
  (`src/core/daemon_documents.rs:709`-`:715`).
  **An authenticated human can NOT bypass the concurrency protection on this path**: the writer
  lease is a daemon-held token, and the disk digest is a content hash, so a broader authorization
  profile does not defeat them.
- Lifetime: request-scoped. The write completes inside the daemon request; a conflict is recorded
  durably on the document (`mark_disk_conflict`, `src/core/daemon_documents.rs:750`) but the
  effect itself is not deferred.
- Reachable callers: TUI editor save (`src/tui/commands/editor.rs:1250` → `controller.save()` →
  `crates/codegg-client/src/document.rs:846` → `CoreRequest::DocumentSave` at
  `crates/codegg-client/src/document.rs:890`); desktop/ACP through the same frontend-neutral
  `CoreRequest`; daemon-internal via the same dispatch.
- Existing coordination (and its limits): writer lease, per-document operation lock, revision
  check, base-disk-digest conflict detection, atomic replace, workspace repository lock
  (`src/core/daemon_documents.rs:735`-`:738`), and a `FileChanged` event
  (`src/core/daemon_documents.rs:768`-`:774`). It does **not** cover: the HTTP write/delete path
  (which takes no lock at all), a linked worktree of the same repository, a second CodeGG process
  (the lock table is in-process — `crates/codegg-core/src/workspace_services.rs:204`-`:207`), an
  external editor or IDE writing the same file, or a run id (the request carries no run
  identifier; only `project_id`, `document_id`, `writer_lease`, `expected_revision`).
- Adversarial bypass example: an external IDE (not CodeGG) saves the same file after the daemon
  read the base digest but before the rename. The daemon's hash check
  (`src/lsp/mutation.rs:226`-`:229`) catches this only if the external write lands *before* the
  read at `src/lsp/mutation.rs:223`. If it lands between that read and the rename at
  `src/lsp/mutation.rs:243`, the daemon's rename overwrites the external edit with no conflict
  reported. The digest check is a check-then-act, not a lock.
- Preliminary disposition: `fenced-native-candidate`. The sink already carries a strong native
  fence; what it lacks is the shared reservation identity (canonical checkout identity covering
  linked worktrees) and a run-scoped permit.
- Evidence: `src/core/daemon_documents.rs:680`, `src/core/daemon_documents.rs:688`,
  `src/core/daemon_documents.rs:697`, `src/core/daemon_documents.rs:709`,
  `src/core/daemon_documents.rs:723`, `src/core/daemon_documents.rs:735`,
  `src/core/daemon_documents.rs:739`, `src/core/daemon_documents.rs:750`,
  `src/core/daemon_documents.rs:768`, `src/lsp/mutation.rs:198`, `src/lsp/mutation.rs:207`,
  `src/lsp/mutation.rs:223`, `src/lsp/mutation.rs:226`, `src/lsp/mutation.rs:234`,
  `src/lsp/mutation.rs:243`, `src/document_service.rs:552`, `src/document_service.rs:568`,
  `src/document_service.rs:618`, `src/document_service.rs:635`, `src/document_service.rs:642`,
  `src/document_service.rs:653`, `src/core/daemon.rs:805`, `src/core/daemon.rs:3812`,
  `crates/codegg-core/src/authorization/policy.rs:128`, `crates/codegg-core/src/workspace.rs:484`,
  `crates/codegg-core/src/workspace.rs:639`, `crates/codegg-core/src/workspace_services.rs:204`,
  `src/tui/commands/editor.rs:1250`, `crates/codegg-client/src/document.rs:846`,
  `crates/codegg-client/src/document.rs:890`

## Sink: daemon document delete

- Final effect owner: `crate::lsp::mutation::checked_workspace_file_delete`
  (`src/core/daemon_documents.rs:560`), whose physical effect is
  `tokio::fs::remove_file(&canonical)` (`src/lsp/mutation.rs:162`). It re-canonicalizes the
  target and requires it to be a regular file under the canonical root
  (`src/lsp/mutation.rs:150`-`:161`).
- Checkout locator: `self.workspaces.resolve(&workspace_id)`
  (`src/core/daemon_documents.rs:525`) → `workspace.canonical_root`; the delete re-canonicalizes
  the root (`src/lsp/mutation.rs:142`) and validates the relative path against it
  (`src/lsp/mutation.rs:146`). Path-canonical identity only; a linked worktree is a distinct root.
- Authority path: daemon request authorization maps `CoreRequest::DocumentDelete` to operation
  `document_delete` with `Capability::FileModify`
  (`crates/codegg-core/src/authorization/policy.rs:143`-`:149`); the handler additionally
  verifies that the project owns the workspace (`verify_project_workspace`,
  `src/core/daemon_documents.rs:528`-`:533`) and refuses to delete a file that is currently
  attached in an editor (`is_path_attached`, `src/core/daemon_documents.rs:537`-`:546`).
  There is no revision lease for a delete — by design, a delete is not an attachment operation.
- Lifetime: request-scoped.
- Reachable callers: daemon `CoreRequest::DocumentDelete`; the TUI reaches it through
  `DocumentSession::delete_file` (`src/tui/document_session.rs:457`-`:468`) →
  `DocumentController::delete_file` (`crates/codegg-client/src/document.rs:527`).
- Existing coordination (and its limits): project/workspace binding check, attached-document
  refusal, path containment, workspace repository lock
  (`src/core/daemon_documents.rs:556`-`:559`). It does **not** cover: the HTTP delete path (no
  lock, no attachment check), a linked worktree, a second CodeGG process, or a run id. It
  publishes no `FileChanged` event and records no edit checkpoint, so the delete is not
  revertible through the checkpoint mechanism.
- Adversarial bypass example: the same file is concurrently deleted over HTTP
  (`src/server/routes/file.rs:276`) and through the daemon document delete. The daemon delete
  re-canonicalizes and checks `is_file` (`src/lsp/mutation.rs:157`), so the second deleter sees
  `Missing`/`Rejected` rather than corrupting state — but the HTTP delete publishes no event and
  takes no lock, so the daemon's `is_path_attached` check
  (`src/core/daemon_documents.rs:539`) can be evaluated against a file the HTTP route has already
  removed, and the TUI editor keeps a writer lease for a path that no longer exists.
- Preliminary disposition: `fenced-native-candidate`.
- Evidence: `src/core/daemon_documents.rs:510`, `src/core/daemon_documents.rs:525`,
  `src/core/daemon_documents.rs:528`, `src/core/daemon_documents.rs:537`,
  `src/core/daemon_documents.rs:556`, `src/core/daemon_documents.rs:560`,
  `src/lsp/mutation.rs:138`, `src/lsp/mutation.rs:142`, `src/lsp/mutation.rs:146`,
  `src/lsp/mutation.rs:150`, `src/lsp/mutation.rs:157`, `src/lsp/mutation.rs:162`,
  `crates/codegg-core/src/authorization/policy.rs:143`,
  `src/tui/document_session.rs:457`, `crates/codegg-client/src/document.rs:527`,
  `src/server/routes/file.rs:276`

## Sink: daemon document create

- Final effect owner: `crate::lsp::mutation::checked_workspace_file_create`
  (`src/core/daemon_documents.rs:463`), whose physical effect is a `create_new` open + write
  (`src/lsp/mutation.rs:100`-`:108`). `create_new(true)` is the real guard: it fails rather than
  truncating, so a file that appears between the existence check
  (`src/lsp/mutation.rs:87`) and the write is not destroyed (`src/lsp/mutation.rs:97`-`:99`).
  Missing parents are created with bounded depth and re-verified containment
  (`src/lsp/mutation.rs:71`-`:75`, `src/lsp/mutation.rs:177`-`:192`).
- Checkout locator: `self.workspaces.resolve(&workspace_id)` → `workspace.canonical_root`;
  the create re-canonicalizes the root (`src/lsp/mutation.rs:76`) and validates the relative path
  against it (`src/lsp/mutation.rs:80`). Path-canonical identity only.
- Authority path: daemon request authorization maps `CoreRequest::DocumentCreate` to operation
  `document_create` with `Capability::FileModify`
  (`crates/codegg-core/src/authorization/policy.rs:134`-`:142`). The create arm additionally
  resolves and verifies the workspace, then acquires the workspace repository lock
  (`src/core/daemon_documents.rs:459`-`:462`). Creating bytes and attaching the replica are two
  separate auditable steps (`src/core/daemon_documents.rs:481`-`:484`).
- Lifetime: request-scoped.
- Reachable callers: daemon `CoreRequest::DocumentCreate`; the TUI reaches it through
  `DocumentSession` (`src/tui/document_session.rs:447`-`:449`) →
  `DocumentController::create` (`crates/codegg-client/src/document.rs:428`).
- Existing coordination (and its limits): capability authorization, workspace verification,
  repository lock, `create_new` no-overwrite semantics, bounded parent creation with
  per-level containment re-verification. It does **not** cover: the HTTP write path (which
  truncates an existing file with no `create_new` semantics), a linked worktree, a second
  CodeGG process, or a run id.
- Adversarial bypass example: an HTTP write creates `foo.rs` at
  `src/server/routes/file.rs:238` at the same moment the daemon creates `foo.rs`. The daemon's
  `create_new` open (`src/lsp/mutation.rs:101`-`:105`) fails with `AlreadyExists` rather than
  clobbering, so the daemon side is safe — but the HTTP side has already truncated whatever was
  at `foo.rs`, and neither side publishes a `FileChanged` event for the create, so the TUI file
  tree and the editor do not learn that a file appeared.
- Preliminary disposition: `fenced-native-candidate`.
- Evidence: `src/core/daemon_documents.rs:414`, `src/core/daemon_documents.rs:459`,
  `src/core/daemon_documents.rs:463`, `src/core/daemon_documents.rs:481`,
  `src/lsp/mutation.rs:63`, `src/lsp/mutation.rs:71`, `src/lsp/mutation.rs:76`,
  `src/lsp/mutation.rs:80`, `src/lsp/mutation.rs:87`, `src/lsp/mutation.rs:97`,
  `src/lsp/mutation.rs:100`, `src/lsp/mutation.rs:177`,
  `crates/codegg-core/src/authorization/policy.rs:134`,
  `src/tui/document_session.rs:447`, `crates/codegg-client/src/document.rs:428`,
  `src/server/routes/file.rs:238`

## Sink: LSP preview apply (host applyEdit path)

- Final effect owner: `crate::lsp::mutation::apply_preview`
  (`src/lsp/mutation.rs:323`). The physical effect is `write_atomic` per file
  (`src/lsp/mutation.rs:443`; implementation `src/lsp/mutation.rs:592`, temp file + `fs::rename`
  at `:599`).
- Checkout locator: `workspace_root.canonicalize()` (`src/lsp/mutation.rs:333`-`:335`). Each patch
  path is validated against the canonical root (`src/lsp/mutation.rs:341`) and reduced to a
  root-relative path (`src/lsp/mutation.rs:343`-`:347`). Path-canonical identity only; a linked
  worktree is a distinct root and a distinct lock.
- Authority path: three gates before the write.
  (1) Document operation gate: `documents.lock_clean_paths(&request.workspace_id, &relative_paths)`
  (`src/lsp/mutation.rs:360`-`:369`) refuses if any open editor buffer for those paths is dirty
  ("dirty" check at `src/document_service.rs:610`-`:613`). The comment at
  `src/lsp/mutation.rs:357`-`:359` states the global order: document gates precede the workspace
  lock, matching the checked-save path.
  (2) Reviewed-digest revalidation: the preview digest is recomputed from the request and
  compared (`src/lsp/mutation.rs:385`-`:401`).
  (3) Pre-write hash check: each file's captured pre-state hash must equal the patch's
  `original_hash` (`src/lsp/mutation.rs:431`-`:436`), and the file must be present
  (`src/lsp/mutation.rs:426`-`:430`).
  Then the workspace repository lock is taken (`src/lsp/mutation.rs:403`), managed-document
  dirtiness is re-checked under it (`src/lsp/mutation.rs:404`-`:413`), and the checkpoint is
  persisted with `workspace_id`, `session_id`, `turn_id`, `batch_seq`
  (`src/lsp/mutation.rs:491`-`:506`).
- Lifetime: request-scoped for the write; the **checkpoint record is durable** (persisted to the
  pool at `src/lsp/mutation.rs:501`) and can later be undone/reapplied by the daemon-internal
  path.
- Reachable callers: (a) the broker tool `src/tool/lsp_preview_apply.rs:226` (model-mediated);
  (b) daemon-internal goal executor `src/core/daemon_goals.rs:1052`. The host `applyEdit` path
  is the daemon-internal one.
- Existing coordination (and its limits): reviewed-digest revalidation, per-file original-hash
  check, dirty-editor gate, workspace repository lock, per-file atomic write, **multi-file
  rollback** on any failure (`src/lsp/mutation.rs:444`, `:454`, `:486`, `:502`;
  `rollback_files` at `src/lsp/mutation.rs:607`), post-state verification
  (`src/lsp/mutation.rs:451`-`:489`), a durable edit checkpoint, and `FileChanged` events
  (`src/lsp/mutation.rs:508`-`:514`). It does **not** cover: the HTTP write/delete path, a
  linked worktree, a second CodeGG process, an external editor's write, or a shared-checkout
  reservation. It carries a `turn_id` but no M005-style run id, and `turn_id` is not a
  reservation identity.
- Adversarial bypass example: an HTTP write to one of the preview's target files lands after
  the preview's `original_hash` check (`src/lsp/mutation.rs:431`) but before `write_atomic`
  (`src/lsp/mutation.rs:443`). The preview then overwrites the HTTP write. The post-state
  verification at `src/lsp/mutation.rs:471` compares against the *planned* new content, not
  against anything the HTTP route did, so the clobber is not detected — it is recorded as a
  successful checkpoint, and the undo path will later "restore" the HTTP write away.
- Preliminary disposition: `fenced-native-candidate`.
- Evidence: `src/lsp/mutation.rs:323`, `src/lsp/mutation.rs:333`, `src/lsp/mutation.rs:341`,
  `src/lsp/mutation.rs:343`, `src/lsp/mutation.rs:357`, `src/lsp/mutation.rs:360`,
  `src/lsp/mutation.rs:385`, `src/lsp/mutation.rs:403`, `src/lsp/mutation.rs:404`,
  `src/lsp/mutation.rs:426`, `src/lsp/mutation.rs:431`, `src/lsp/mutation.rs:443`,
  `src/lsp/mutation.rs:444`, `src/lsp/mutation.rs:451`, `src/lsp/mutation.rs:471`,
  `src/lsp/mutation.rs:491`, `src/lsp/mutation.rs:501`, `src/lsp/mutation.rs:508`,
  `src/lsp/mutation.rs:592`, `src/lsp/mutation.rs:599`, `src/lsp/mutation.rs:607`,
  `src/document_service.rs:610`, `src/tool/lsp_preview_apply.rs:226`,
  `src/core/daemon_goals.rs:1052`

## Sink: project init AGENTS.md publish

- Final effect owner: `publish_checked` (`src/core/daemon_project_init.rs:347`). Physical effect:
  a `create_new` temp file + write (`src/core/daemon_project_init.rs:365`-`:371`), then either
  `fs::rename` (update path, `:380`) or `fs::hard_link` (create path, `:383` — the hard link is
  the exactly-one-winner guard, confirmed by the concurrency test at
  `src/core/daemon_project_init.rs:481`-`:490`). The root is canonicalized inside
  `publish_checked` (`src/core/daemon_project_init.rs:355`-`:357`).
- Checkout locator: `resolve_project_workspace` (`src/core/daemon_project_init.rs:179`-`:185`)
  returns the workspace's `canonical_root` (`src/core/daemon_project_init.rs:286`). The publish
  re-checks that the resolved root still equals the root the draft was issued against
  (`src/core/daemon_project_init.rs:186`-`:191`) and refuses a stale preview. Path-canonical
  identity only; a linked worktree is a distinct root.
- Authority path: a single-use, scope-bound, expiring draft token
  (`src/core/daemon_project_init.rs:154`-`:178`; the registry is
  `src/core/project_init.rs:37`-`:89` and is in-memory, so a daemon restart discards unpublished
  drafts — see the test at `src/core/project_init.rs:145`). The target is always the literal
  `AGENTS.md` at the workspace root (`src/core/daemon_project_init.rs:69`, `:358`) — this sink
  cannot be aimed at an arbitrary path. Daemon request authorization maps
  `CoreRequest::ProjectInitPublish` to operation `project_init_publish` with
  `Capability::FileModify` (`crates/codegg-core/src/authorization/policy.rs:155`-`:159`).
  The write is additionally guarded by an exact expected digest
  (`publish_checked` → `read_observed_target`, `src/core/daemon_project_init.rs:359`;
  error text at `:298`), so a stale or externally-edited `AGENTS.md` fails closed
  (test at `src/core/daemon_project_init.rs:462`-`:478`).
  A dirty editor buffer on `AGENTS.md` is refused
  (`src/core/daemon_project_init.rs:192`-`:204`).
- Lifetime: request-scoped for the write. The **draft** is deferred and time-limited (24h per
  the module contract; the registry is `src/core/project_init.rs:33`-`:89`), and it is discarded
  on daemon restart.
- Reachable callers: daemon `CoreRequest::ProjectInitPublish`; frontend-neutral, reached by the
  TUI/desktop via the same request. Not reachable by the model ToolBroker.
- Existing coordination (and its limits): single-use scope-bound draft token, stale-root
  recheck, dirty-editor gate, workspace repository lock
  (`src/core/daemon_project_init.rs:211`), exact-digest guard, atomic replace / hard-link
  create, permission preservation (`src/core/daemon_project_init.rs:372`-`:375`), and a
  `FileChanged` event (`src/core/daemon_project_init.rs:220`-`:229`). It does **not** cover:
  the HTTP write path, a linked worktree, a second CodeGG process, or a run id. It publishes
  **no** edit checkpoint, so the AGENTS.md publication is not revertible through the checkpoint
  mechanism.
- Adversarial bypass example: an HTTP write replaces `AGENTS.md` between the draft's digest
  capture and the publish. `publish_checked` re-reads the target and compares the digest
  (`src/core/daemon_project_init.rs:359`), so the publish fails closed with
  `project_init_publish_failed` — this sink is *not* the weak link. The weak link is the
  converse: the HTTP write is what silently clobbers an `AGENTS.md` that CodeGG just published,
  and because the HTTP path publishes no `FileChanged` event, the editor and the file tree never
  learn that the published content was replaced.
- Preliminary disposition: `fenced-native-candidate`.
- Evidence: `src/core/daemon_project_init.rs:147`, `src/core/daemon_project_init.rs:154`,
  `src/core/daemon_project_init.rs:160`, `src/core/daemon_project_init.rs:179`,
  `src/core/daemon_project_init.rs:186`, `src/core/daemon_project_init.rs:192`,
  `src/core/daemon_project_init.rs:211`, `src/core/daemon_project_init.rs:220`,
  `src/core/daemon_project_init.rs:286`, `src/core/daemon_project_init.rs:298`,
  `src/core/daemon_project_init.rs:347`, `src/core/daemon_project_init.rs:355`,
  `src/core/daemon_project_init.rs:359`, `src/core/daemon_project_init.rs:365`,
  `src/core/daemon_project_init.rs:372`, `src/core/daemon_project_init.rs:380`,
  `src/core/daemon_project_init.rs:383`, `src/core/project_init.rs:37`,
  `src/core/project_init.rs:89`, `src/core/project_init.rs:145`,
  `crates/codegg-core/src/authorization/policy.rs:155`

## Sink: edit-checkpoint undo / reapply (the live snapshot restore)

- Final effect owner: `EditCheckpointManager::checked_restore`
  (`crates/codegg-core/src/snapshot/checkpoint.rs:653`) →
  `checked_restore_inner` (`crates/codegg-core/src/snapshot/checked_restore.rs:355`) →
  `apply_file_state` (`crates/codegg-core/src/snapshot/checked_restore.rs:145`), whose physical
  effect is `restore_file_checked` (`crates/codegg-core/src/snapshot/checked_restore.rs:220` for
  unix, `:293` for the other platform).
- Checkout locator: the manager is constructed with `record.canonical_root`
  (`src/core/daemon_goals.rs:824`-`:827`; the same construction for the other three arms at
  `src/core/daemon_goals.rs:872`-`:875`, `:923`-`:926`, `:973`-`:976`).
  `checked_restore_inner` re-canonicalizes the root
  (`crates/codegg-core/src/snapshot/checked_restore.rs:151`) and validates every stored path with
  `validate_checkpoint_path` (`crates/codegg-core/src/snapshot/checked_restore.rs:128`, applied at
  `:364` and again at `crates/codegg-core/src/snapshot/checkpoint.rs:707`-`:711`). Path-canonical
  identity only; a linked worktree is a distinct root, and the restore does not consult git
  lineage.
- Authority path: the checkpoint must match the caller's workspace
  (`crates/codegg-core/src/snapshot/checkpoint.rs:667`-`:678` → `WrongWorkspace`) and, when
  supplied, the caller's session (`:680`-`:692` → `WrongSession`); both denials are audited via
  `log_restore_operation` (`:674`-`:676`, `:687`-`:689`). The daemon arms pass
  `Some(&session_id)` (`src/core/daemon_goals.rs:829`, `:876`, `:930`, `:977`), so a restore is
  session-bound. The workspace repository lock is taken **before** the restore and held across it
  (`src/core/daemon_goals.rs:820`-`:823`).
- Lifetime: **deferred and persistent** in the sense that the restore replays a *durable
  checkpoint* recorded by an earlier write. The restore operation itself is request-scoped, but
  the artifact it replays outlives the request.
- Reachable callers: daemon `CoreRequest::EditCheckpointUndo`
  (`src/core/daemon_goals.rs:793`), `EditCheckpointUndoLatest` (`:844`),
  `EditCheckpointReapply` (`:894`), `EditCheckpointReapplyLatest` (`:945`). Frontend-neutral
  daemon requests; not model-mediated and not HTTP-routed.
- Existing coordination (and its limits): workspace + session binding checks, per-path
  validation, **conflict detection** — the current on-disk state of every path must equal the
  checkpoint's expected state or the whole restore fails closed with a bounded conflict list
  (`crates/codegg-core/src/snapshot/checked_restore.rs:411`-`:427`), workspace repository lock
  held across the apply, and an audit log for every denial. It does **not** cover: the HTTP
  write path, a linked worktree, a second CodeGG process, or the HTTP path's lack of any
  checkpoint (an HTTP write is invisible to the conflict detector, so it is simply overwritten).
  Partial application is possible: `checked_restore_inner` applies files sequentially and
  **stops at the first I/O error**, returning `PartialFailure` with the applied and failed path
  lists (`crates/codegg-core/src/snapshot/checked_restore.rs:434`-`:455`) — there is no rollback
  for a partially applied restore.
- Adversarial bypass example: the HTTP write changes a checkpointed file to content that happens
  to differ from the checkpoint's expected state. The restore then fails closed with
  `Conflict` — correct. The dangerous variant is the opposite ordering: the restore applies
  first (winning the lock it holds), then the HTTP write lands with no lock and no digest, and
  the checkpoint's post-state no longer matches disk. The next `EditCheckpointUndo` will report
  `Conflict` (safe), but the user has lost the restore with no explanation, because the HTTP
  write published no `FileChanged` event.
- Preliminary disposition: `fenced-native-candidate`.
- Evidence: `crates/codegg-core/src/snapshot/checkpoint.rs:653`, `crates/codegg-core/src/snapshot/checkpoint.rs:667`,
  `crates/codegg-core/src/snapshot/checkpoint.rs:680`, `crates/codegg-core/src/snapshot/checkpoint.rs:707`,
  `crates/codegg-core/src/snapshot/checked_restore.rs:128`, `crates/codegg-core/src/snapshot/checked_restore.rs:145`,
  `crates/codegg-core/src/snapshot/checked_restore.rs:151`, `crates/codegg-core/src/snapshot/checked_restore.rs:220`,
  `crates/codegg-core/src/snapshot/checked_restore.rs:293`, `crates/codegg-core/src/snapshot/checked_restore.rs:355`,
  `crates/codegg-core/src/snapshot/checked_restore.rs:364`, `crates/codegg-core/src/snapshot/checked_restore.rs:411`,
  `crates/codegg-core/src/snapshot/checked_restore.rs:434`,
  `src/core/daemon_goals.rs:793`, `src/core/daemon_goals.rs:820`,
  `src/core/daemon_goals.rs:829`, `src/core/daemon_goals.rs:844`,
  `src/core/daemon_goals.rs:894`, `src/core/daemon_goals.rs:945`

## Sink: legacy SnapshotManager restore (codegg-core snapshot module)

- Final effect owner: `SnapshotManager::restore`
  (`crates/codegg-core/src/snapshot/mod.rs:283`) and `restore_to_path`
  (`crates/codegg-core/src/snapshot/mod.rs:323`), which writes through `restore_file`
  (`crates/codegg-core/src/snapshot/mod.rs:408`) / `restore_file_unix`
  (`crates/codegg-core/src/snapshot/mod.rs:448`).
- Checkout locator: the manager is constructed with a root path
  (`crates/codegg-core/src/snapshot/mod.rs`, `SnapshotManager::new_with_options`); the agent loop
  builds one from the workspace root (`src/agent/loop.rs:203`, `:214`). Path-based.
- Authority path: **none established.** The module's own restore entry points carry no principal,
  no capability check, no revision lease, and no run id in their signatures. `restore_to_path`
  does reject traversal components (`crates/codegg-core/src/snapshot/mod.rs:335`).
- Lifetime: request-scoped when called.
- Reachable callers: **none in production.** `SnapshotManager::restore` / `restore_to_path` are
  called only from `tests/snapshot.rs` (`:294`) and `tests/edit_checkpoint_integration.rs:743`
  constructs the manager. The agent loop only ever *captures*
  (`src/agent/snapshot_capture.rs:18`-`:20`, `:212`, `:221`-`:222`); it never restores.
- Existing coordination (and its limits): traversal rejection on `restore_to_path`
  (`crates/codegg-core/src/snapshot/mod.rs:335`). No lock, no lease, no digest, no checkpoint
  round-trip. It is **not** the mechanism behind the live undo/reapply path — that is the
  `EditCheckpointManager` sink above.
- Adversarial bypass example: not applicable today, because there is no production caller. If
  this API were ever wired to a recovery or admin path, it would be a completely unfenced
  project-root writer: it would write files with no repository lock, no digest check, and no
  `FileChanged` event, overlapping the LSP preview apply and the daemon document save.
- Preliminary disposition: `out-of-contract` — a dormant, unreferenced API in the snapshot
  module. It is **not** the "snapshot restore" sink the C004 plan means; the live one is the
  `EditCheckpointManager` sink above. It should not be counted as reachable coverage, and it
  should not be silently deleted as dead code either (it is exercised by the test suite).
- Evidence: `crates/codegg-core/src/snapshot/mod.rs:283`, `crates/codegg-core/src/snapshot/mod.rs:323`,
  `crates/codegg-core/src/snapshot/mod.rs:335`, `crates/codegg-core/src/snapshot/mod.rs:408`,
  `crates/codegg-core/src/snapshot/mod.rs:448`, `src/agent/loop.rs:203`,
  `src/agent/loop.rs:214`, `src/agent/snapshot_capture.rs:18`,
  `src/agent/snapshot_capture.rs:212`, `src/agent/snapshot_capture.rs:221`,
  `tests/snapshot.rs:294`, `tests/edit_checkpoint_integration.rs:743`

## Sink: TUI editor save (frontend entry point)

- Final effect owner: **not a separate final effect owner.** `save_editor`
  (`src/tui/commands/editor.rs:1250`) calls `controller.save()`
  (`src/tui/commands/editor.rs:1257`), which is
  `DocumentController::save` (`crates/codegg-client/src/document.rs:846`) and issues
  `CoreRequest::DocumentSave` (`crates/codegg-client/src/document.rs:890`). The physical effect
  is the same `checked_workspace_text_write` recorded under "daemon document save" above.
- Checkout locator: the controller does not resolve a root; the daemon resolves it from the
  attachment's `workspace_id` (`src/core/daemon_documents.rs:723`) to `workspace.canonical_root`.
  The TUI therefore cannot aim the write at an arbitrary path.
- Authority path: the client-side controller holds the daemon-issued `writer_lease`
  (`crates/codegg-client/src/document.rs:862`-`:864`) and the daemon revision
  (`crates/codegg-client/src/document.rs:865`), and sends both with the save request
  (`crates/codegg-client/src/document.rs:890`-`:895`). The daemon re-validates both
  (`src/core/daemon_documents.rs:688`, `src/document_service.rs:635`-`:644`). The controller
  also refuses to save a conflicted document (`crates/codegg-client/src/document.rs:856`-`:858`).
  **An authenticated human cannot bypass the concurrency protection on this path.**
- Lifetime: request-scoped, with the draft held client-side until flush
  (`crates/codegg-client/src/document.rs:847`).
- Reachable callers: the TUI editor's save command
  (`src/tui/commands/editor.rs:1250`); desktop/ACP use the same `DocumentController`.
- Existing coordination (and its limits): everything the daemon document save provides, plus a
  client-side conflict guard and a lifecycle epoch guard against a stale completion
  (`crates/codegg-client/src/document.rs:909`-`:911`). The documented contract is that a disk
  conflict is surfaced with the retained draft and never auto-reloaded or auto-discarded
  (`src/tui/commands/editor.rs:1246`-`:1249`). It does **not** cover: the HTTP write/delete
  path, a linked worktree, a second CodeGG process, or an external editor's write.
- Adversarial bypass example: the human edits a file in the TUI while an HTTP write or an
  external IDE saves the same file. The TUI save's base-digest check
  (`src/lsp/mutation.rs:226`-`:229`) catches the HTTP/IDE write only if it lands *before* the
  daemon's read of the current bytes (`src/lsp/mutation.rs:223`); if it lands in the window
  between that read and the rename (`src/lsp/mutation.rs:243`), the TUI save silently wins and
  the external edit is lost with no conflict shown to the human.
- Preliminary disposition: `fenced-native-candidate` (entry point; the fence is the daemon
  document save's). This sink is explicitly **human-originated** and, per the C001 contract, must
  be treated equally with agent writes — it must not receive a broader approval profile to
  escape a shared-checkout reservation.
- Evidence: `src/tui/commands/editor.rs:1250`, `src/tui/commands/editor.rs:1257`,
  `src/tui/commands/editor.rs:1246`, `crates/codegg-client/src/document.rs:846`,
  `crates/codegg-client/src/document.rs:856`, `crates/codegg-client/src/document.rs:862`,
  `crates/codegg-client/src/document.rs:890`, `crates/codegg-client/src/document.rs:909`,
  `src/core/daemon_documents.rs:688`, `src/core/daemon_documents.rs:723`,
  `src/document_service.rs:635`, `src/document_service.rs:642`,
  `src/lsp/mutation.rs:223`, `src/lsp/mutation.rs:226`, `src/lsp/mutation.rs:243`

## Sink: IDE temporary diff copies

- Final effect owner: `tempfile::Builder` temp files — `src/ide/mod.rs:136`-`:143` (vscode),
  `:190`-`:195` (jetbrains), `:276`-`:281` and `:334`-`:339` (generic). The cleanup sweep scans
  `std::env::temp_dir()` (`src/ide/mod.rs:70`-`:71`).
- Checkout locator: **none.** The module never takes a workspace root; it receives two file
  paths as arguments (`open_diff`, `src/ide/mod.rs:98`-`:108`) and writes only into the system
  temp directory. No `canonicalize`, no root join, no path-policy call appears in the module.
- Authority path: none required — the write target is not attacker-influenced and not in the
  checkout.
- Lifetime: ephemeral. The temp files are removed by the `TempFilesGuard` cleanup pass over
  `std::env::temp_dir()` (`src/ide/mod.rs:70`-`:71`).
- Reachable callers: the TUI diff-open action.
- Existing coordination (and its limits): the diff tool is launched with content copied into
  temp files, so the external IDE can never write back into the checkout by editing the diff
  target. Nothing to coordinate.
- Adversarial bypass example: none available — this sink cannot touch the checkout.
- Preliminary disposition: `private-scratch`. It is **not** a project-root writer and needs no
  writer permit. This confirms the C004 plan's assumption that "src/ide/mod.rs temporary diff
  copies … need no checkout writer permit if they truly cannot modify the root" — verified
  true at this SHA.
- Evidence: `src/ide/mod.rs:70`, `src/ide/mod.rs:98`, `src/ide/mod.rs:104`,
  `src/ide/mod.rs:136`, `src/ide/mod.rs:140`, `src/ide/mod.rs:190`,
  `src/ide/mod.rs:194`, `src/ide/mod.rs:276`, `src/ide/mod.rs:280`,
  `src/ide/mod.rs:334`, `src/ide/mod.rs:338`

## Coverage gaps

- **`.codegg/runs` run-artifact store — a project-root writer NOT in the assigned list.** The run
  root is `workspace.canonical_root.join(".codegg").join("runs")` — *inside the checkout*
  (`crates/codegg-core/src/workspace_services.rs:406`), its parent is created with
  `std::fs::create_dir_all` (`:408`-`:417`), and it is handed to `FsRunStore::new`
  (`:418`). `FsRunStore` then creates the root (`crates/codegg-core/src/run_store.rs:1161`),
  renames index files into place (`:1236`, `:1257`), creates run directories (`:1439`), and
  renames manifests (`:1557`). The store's own mutex (`crates/codegg-core/src/run_store.rs:1147`)
  is an in-process lock over one store instance, not a checkout reservation. This is a
  project-root writer with no writer permit.
- **Legacy `<project_root>/.codegg/sessions.db` — a project-root writer NOT in the assigned
  list.** `init_legacy_project_store` targets
  `project_root.join(".codegg").join("sessions.db")`
  (`crates/codegg-core/src/storage/mod.rs:145`) and `init_migrated_legacy_project_store` applies
  the schema migration to that same in-checkout path (`:163`-`:171`); `init_pool_at` creates the
  `.codegg` parent directory (`:175`-`:191`). The CLI calls the migrating variant at
  `src/main.rs:1775`, `:1816`, `:1878`, `:1920`, `:3159`, `:4224`. The legacy importer
  (`crates/codegg-core/src/migration.rs:48`-`:55`, `:128`-`:147`) opens the in-checkout source
  pool (`init_legacy_project_store`, `crates/codegg-core/src/storage/mod.rs:144`). The current
  daemon catalog is user-scoped, not in the checkout
  (`crates/codegg-core/src/storage/mod.rs:109`-`:121`), so the in-checkout DB is a **legacy**
  path that still runs.
- **Both of the above are startup/recovery paths.** They run at daemon start, workspace
  activation, or CLI invocation — **before any model turn exists**, so they have no run id and
  cannot be attributed to a run. They are exactly the "protect startup/recovery paths that run
  before a model turn and aren't aware of run IDs" case in C004 work package D. A pre-effect
  reservation check is the only viable fence shape for them; a run-scoped permit is not
  expressible.
- **The TUI file tree has no checkout write of its own.** `src/tui/commands/file_tree.rs` and
  `src/tui/app/state/file_tree.rs` contain no production `fs::write` / `remove_file` / `rename`
  against a workspace root (the only such calls in those files are test fixtures at
  `src/tui/commands/file_tree.rs:923`, `:979`, `:1122` and
  `src/tui/app/state/file_tree.rs:586`-`:587`). The TUI reaches the checkout exclusively through
  the document controller. Read-only file-tree listing needs no writer permit.
- **Agent `AssetRefresh` does not write to the checkout.** `src/agent/asset_refresh.rs` publishes
  an in-memory snapshot (`:243`, `:245`); the only `fs::write` / `create_dir_all` calls in
  `src/agent/asset_refresh.rs` and `src/agent/asset_snapshot_builder.rs` are test fixtures
  (`:603`-`:604`, `:623`, `:659`-`:660` and `:296`-`:297`, `:318`). It is not a checkout writer.
- **Git mutations are broker-mediated, not non-broker.** `GitMutationExecutor` is reached from
  `src/tool/git.rs:532`, `:940`, `src/tool/commit.rs:58`, `src/tool/bash/process.rs:1029`,
  `src/git_network_ops.rs:96`+ and `src/git_mutations_ops.rs:34`+ — all under `src/tool/`, i.e.
  the ToolBroker boundary. They are therefore out of *this* sink class, though they are a
  competing writer on the same checkout and a shared reservation must still be visible to them.
- **UNPROVEN: whether the C002 permit machinery, the "final writer service", or a
  shared-checkout reservation primitive already exists in the tree at this SHA.** This audit
  read the writer surfaces and their coordination, not the C002/C003 implementation, because
  the assigned scope is the non-broker writer sinks. The sibling plan documents
  `001-restricted-profile-contract-and-writer-matrix.md` … `006-…` in
  `plans/implementation/agent-workstream-m005-writer-containment-corrective/` describe the
  intended contract, and C004 is marked "blocked — C003 strict closure" in
  `004-nonbroker-writer-sink-mediation.md:3`.
- **UNPROVEN: whether `ServerState.daemon` (`src/server/state.rs:111`) is populated in the
  deployed configuration, and therefore whether the HTTP server process is the same process as
  the daemon.** This does not change the HTTP disposition: the HTTP handlers never consult
  `state.daemon` and never take a `WorkspaceLockTable`, so they are unfenced in either
  configuration.
- **UNPROVEN: the exact git-lineage mapping for a linked worktree whose `.git` is a *file*.**
  `crates/codegg-core/src/project_catalog.rs` stores a `lineage_key`
  (`:456`) and a `common_directory` column (`:777`, `:1728` inserts it as `NULL`), but no writer
  sink reads either. `WorkspaceRegistry::register` canonicalizes the path only
  (`crates/codegg-core/src/workspace.rs:484`, `:615`, `:639`), and
  `WorkspaceLockTable::acquire_repository` keys the lock on the canonicalized path
  (`crates/codegg-core/src/workspace_services.rs:219`-`:223`). A linked worktree therefore
  resolves to a **different** canonical root, a different workspace id, and a different lock —
  the same repository reached through two worktrees is currently two independent writer
  domains. Whether a git common-dir resolver exists elsewhere was not exhaustively searched.

## Cross-cutting notes

- **The final effect owner is not the frontend button.** Four distinct entry points
  (TUI editor save, HTTP write, HTTP delete, LSP preview apply) and two daemon-internal ones
  (checkpoint undo/reapply, project init publish) collapse onto **two** physical write
  primitives in `src/lsp/mutation.rs` (`checked_workspace_text_write` at `:198`,
  `checked_workspace_file_create` at `:63`, `checked_workspace_file_delete` at `:138`) plus
  `publish_checked` in `src/core/daemon_project_init.rs:347` and `apply_file_state` in
  `crates/codegg-core/src/snapshot/checked_restore.rs:145`. The HTTP write and delete are the
  only project-root writers that bypass those primitives entirely.
- **Human-originated writes are not second-class today — they are *unfenced*.** The TUI editor
  save carries a writer lease, a revision, a base-disk digest, an atomic replace and a
  repository lock. The HTTP write and delete carry a project capability and nothing else. Both
  are human-originated. Under the C001 contract they must be treated equally, which today means
  the HTTP path is the one that needs the fence adopted, not the one that needs a broader
  approval profile.
- **The authorization axis and the concurrency axis are orthogonal, and the HTTP path conflates
  them.** `authorize_project` with `Capability::FileModify`
  (`src/server/authz.rs:422`) answers "may this principal modify this project?". It does not
  answer "may this write proceed now, given what else is writing?". A `LocalOwner` principal —
  which the global bearer and the auth-disabled env both produce
  (`src/server/middleware/auth.rs:100`-`:114`, `:62`-`:67`) — passes the capability check on
  every project. That is correct behavior for authorization and is *not* a concurrency
  exemption. The corrective must not widen the authorization surface to resolve a contention
  problem.
- **The repository lock is in-process and path-keyed.**
  `WorkspaceLockTable` is a `DashMap<PathBuf, Arc<AsyncMutex<()>>>`
  (`crates/codegg-core/src/workspace_services.rs:204`-`:207`) keyed by
  `tokio::fs::canonicalize(repo_root)` (`:219`-`:223`), and a **fresh table is built per
  workspace** (`:420`). Consequences: (a) it does not serialize a second CodeGG process against
  the first; (b) it does not serialize two distinct workspace ids that happen to name the same
  directory; (c) it does not serialize a linked worktree against its main checkout. None of
  these is a defect in the current design — the table is explicitly a Phase-3 Git-scoped
  primitive (`crates/codegg-core/src/workspace_services.rs:195`-`:203`) — but it means the
  existing "coordination" is a **same-process, same-path** mutual exclusion and nothing more.
  A shared-checkout reservation must be keyed on a **canonical checkout identity** that covers
  linked worktrees (git common dir) and path aliases, and must be durable across processes if
  it is to fence cross-process contention.
- **A path alias does resolve to the same identity; a linked worktree does not.** Workspace
  registration canonicalizes the root (`crates/codegg-core/src/workspace.rs:484`, `:615`,
  `:639`) and the registry is keyed by that canonical root
  (`crates/codegg-core/src/workspace.rs:503`, `:517`). A symlinked path to the same directory
  therefore yields the same workspace id and the same lock key. A linked worktree is a
  genuinely different directory and yields a different one. This is the precise line C004 work
  package A draws ("linked worktrees with a .git file and alternate path aliases must map to
  same canonical identity"): aliases already do; **linked worktrees do not**.
- **The document-op gate ordering is explicit and must be preserved.** `apply_preview` states
  the global order — document operation gates precede the workspace lock
  (`src/lsp/mutation.rs:357`-`:359`) — and the checked-save path takes the per-document
  operation lock (`src/core/daemon_documents.rs:688`) before the repository lock
  (`src/core/daemon_documents.rs:735`-`:738`). Project init takes the document gate
  (`src/core/daemon_project_init.rs:192`-`:204`) before the repository lock (`:211`). Any new
  fence must respect this order to avoid the lock inversion the C004 acceptance matrix calls
  out.
- **Three of the six live sinks already know a `turn_id` or `session_id`, none knows a run id.**
  `apply_preview` records `session_id` and `turn_id` on the checkpoint
  (`src/lsp/mutation.rs:494`-`:495`), and the checkpoint undo/reapply arms are session-bound
  (`src/core/daemon_goals.rs:829`). The document save request carries `project_id`,
  `document_id`, `writer_lease`, `expected_revision` and no run identifier
  (`src/core/daemon_documents.rs:680`-`:685`). The HTTP request carries only
  `path`/`content` plus project/workspace/directory
  (`src/server/routes/file.rs:108`-`:115`). The project-init publish carries a draft token
  scoped to project/workspace/client (`src/core/daemon_project_init.rs:160`-`:178`). The
  startup/recovery writers identified under Coverage gaps carry no identifier at all.
- **Answer to the coverage question — is any CodeGG project-root write API missing from this
  inventory?** **Yes. At least two, and they are project-local metadata writers rather than
  user-file writers.**
  (1) **`.codegg/runs`** — the run-artifact store rooted at
  `workspace.canonical_root.join(".codegg").join("runs")`
  (`crates/codegg-core/src/workspace_services.rs:406`, `:418`), which creates directories and
  renames files inside the checkout
  (`crates/codegg-core/src/run_store.rs:1161`, `:1236`, `:1257`, `:1439`, `:1557`).
  (2) **Legacy `<project_root>/.codegg/sessions.db`** — opened and schema-migrated inside the
  checkout (`crates/codegg-core/src/storage/mod.rs:145`, `:163`-`:171`, `:175`-`:191`; CLI
  callers at `src/main.rs:1775`, `:1816`, `:1878`, `:1920`, `:3159`, `:4224`; importer at
  `crates/codegg-core/src/migration.rs:48`-`:55`, `:128`-`:147`).
  **How this was checked.** I enumerated every filesystem-mutation primitive in `src/` with a
  content search for `fs::write(`, `fs::remove_file(`, `fs::remove_dir`, `fs::rename(`,
  `fs::create_dir_all(`, `fs::copy(`, `persist(`, `NamedTempFile` and `Builder::new()` across
  `src/**/*.rs`, then classified every hit by target location. The project-root writers that
  survived classification are exactly the sinks in this document, plus the two metadata writers
  above. Separately I traced the *backend* effect owners rather than the frontend call sites
  (as C004 work package A requires): `CoreRequest` variants that mutate the checkout
  (`DocumentCreate`, `DocumentDelete`, `DocumentSave`, `DocumentReload`, `ProjectInitPublish`,
  `EditCheckpoint*`) were resolved through
  `crates/codegg-core/src/authorization/policy.rs:101` (`operation_descriptor`) to confirm the
  set of authorized write operations, and each was traced to its final effect owner. The
  remaining `src/` hits resolve to non-checkout locations: the TUI state root
  (`src/tui/app/state/persistence.rs:160`), the permission store, the MCP token store, the
  plugin directory, the research store, the tool-program ledger/source directories, the
  managed-process journal, the training-data store, and the `tempfile` scratch space used by
  `src/ide/mod.rs`. `src/core/project_init.rs` is an in-memory draft registry with no filesystem
  write. `src/git_mutations.rs` is reached only from `src/tool/*` (broker-mediated).
  **What this does not establish:** the classification is a static sweep of the current tree at
  this SHA; a `grep` for write primitives cannot prove the *absence* of a writer that reaches
  the filesystem through a differently-spelled call (e.g. `OpenOptions::open` + `write_all`,
  which is how `src/lsp/mutation.rs` and `src/core/daemon_project_init.rs` actually write).
  Those two were found by tracing the effect owners rather than by the sweep. A guard (C004
  work package E) is still required to make the inventory reliable over time.
