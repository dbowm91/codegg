# C002 evidence 01 — Worktree lease and generation model

Read-only source audit for milestone **C002** (durable fenced checkout writer permit),
scoped to `crates/codegg-core/src/worktree_service.rs`, `crates/codegg-core/src/worktree.rs`,
and `crates/codegg-core/src/workspace.rs` (workspace/project/repository identity only).

Baseline: worktree `work/agent-workstream-m005-writer-audit` at the audit checkout
`/home/sugarwookie/projects/codegg/.worktrees/m005-writer-audit`.
All `file:line` citations are relative to that checkout root.

Audit stance: report what exists. No design is proposed here. Where a fact could not be
established from source, it is marked **UNPROVEN** with the reason.

---

## 1. What is a `WorktreeId`? How is it minted, stored, and looked up?

`WorktreeId` is an **opaque UUIDv4 string**, not an integer and not path-derived.

- Declared through the shared `typed_identity!` macro:
  `crates/codegg-core/src/identity.rs:228-232` (`typed_identity!(WorktreeId, "worktree_id")`).
- Minted by the macro's `new()`: `crates/codegg-core/src/identity.rs:113-114`
  → `Self(uuid::Uuid::new_v4().to_string())`.
- The module header states the design intent explicitly: identities "are generated as
  UUIDv4 values, but persisted values are validated by the shared lexical contract rather
  than by UUID parsing", and "Paths are locators and are never accepted as identity input"
  (`crates/codegg-core/src/identity.rs:1-7`).
- The lexical contract rejects path-like values: a `/` or `\` yields
  `IdentityParseReason::PathLike` (`crates/codegg-core/src/identity.rs:84-85`); the full
  validation is `validate_identity` at `crates/codegg-core/src/identity.rs:77-103`.
  `WorktreeId::parse` is the validating constructor (`crates/codegg-core/src/identity.rs:118-121`).

**Minting site (the only producer in the worktree service):**
`crates/codegg-core/src/worktree_service.rs:821` → `let worktree_id = WorktreeId::new();`.
The id then *determines* the locator rather than being derived from it:
- branch name `codegg/worktree/<first 12 hex chars>` —
  `crates/codegg-core/src/worktree_service.rs:822-825` (validated by `BranchName::new`).
- managed directory `wt-<first 12 hex chars>` —
  `crates/codegg-core/src/worktree_service.rs:827` (and recomputed at
  `crates/codegg-core/src/worktree_service.rs:845` after the managed root is canonicalized).

**Storage:** `managed_worktree.worktree_id TEXT PRIMARY KEY`
(`crates/codegg-core/src/session/schema.rs:2101`), bound by
`PRIMARY KEY(worktree_id, generation)` on the lease table
(`crates/codegg-core/src/session/schema.rs:2128`).
Rows are written by `SqliteWorktreeStore::insert_reserved`
(`crates/codegg-core/src/worktree_service.rs:561-571`) and read back with
`WorktreeId::parse` (`crates/codegg-core/src/worktree_service.rs:512-513`).

**Lookup:** `WorktreeService::get` (`crates/codegg-core/src/worktree_service.rs:787-792`)
→ `WorktreeStore::get` (`:573-575`) → `load_sqlite`
(`crates/codegg-core/src/worktree_service.rs:544-557`),
`SELECT … FROM managed_worktree WHERE worktree_id = ?`.
The in-memory store resolves the same key via a `HashMap<WorktreeId, WorktreeRecord>`
(`crates/codegg-core/src/worktree_service.rs:313`, `:348-350`).

Note: the `managed_worktree` row is **never deleted** — a grep for
`DELETE FROM managed_worktree` across the tree returns nothing; retirement is expressed as
`state = 'removed'` (`crates/codegg-core/src/worktree_service.rs:1112`).

---

## 2. What exactly is a "worktree lease"? Where is it persisted? What fields?

A lease is a **durable, generation-fenced ownership record for one worktree by one
`AgentRunId`**, with a persisted append-only history.

**Type:** `WorktreeLease` — `crates/codegg-core/src/worktree_service.rs:137-145`.

Fields (exactly six):
| Field | Type | Line |
|---|---|---|
| `worktree_id` | `WorktreeId` | `:139` |
| `owner_run_id` | `AgentRunId` | `:140` |
| `generation` | `u64` | `:141` |
| `acquired_at` | `i64` (ms epoch) | `:142` |
| `renewed_at` | `i64` (ms epoch) | `:143` |
| `released_at` | `Option<i64>` | `:144` |

**There is no expiry, deadline, TTL, or lease-state field.** A grep for
`expir|deadline|ttl|timeout` in `crates/codegg-core/src/worktree_service.rs` returns
**no matches**. The lease's liveness is expressed entirely by `released_at: None` plus the
*record's* `owner_run_id`/`lease_generation` columns.

**Persistence (SQLite):** table `worktree_lease` — `crates/codegg-core/src/session/schema.rs:2120-2131`:
```
worktree_id  TEXT NOT NULL
owner_run_id TEXT NOT NULL
generation   INTEGER NOT NULL CHECK (generation > 0)
acquired_at  INTEGER NOT NULL
renewed_at   INTEGER NOT NULL
released_at  INTEGER
PRIMARY KEY(worktree_id, generation)
FOREIGN KEY(worktree_id) REFERENCES managed_worktree(worktree_id) ON DELETE CASCADE
```
plus a **partial unique index that admits at most one active lease per worktree**:
`crates/codegg-core/src/session/schema.rs:2132`
— `CREATE UNIQUE INDEX … idx_worktree_active_lease ON worktree_lease(worktree_id) WHERE released_at IS NULL`.
That index is the durable single-holder guarantee.

The **owner** is also denormalized onto the record row: `managed_worktree.owner_run_id`
(`crates/codegg-core/src/session/schema.rs:2114`) and `managed_worktree.lease_generation`
(`crates/codegg-core/src/session/schema.rs:2113`).

Writers: acquire inserts the lease row —
`crates/codegg-core/src/worktree_service.rs:648`;
renew updates `renewed_at` — `:669`;
release stamps `released_at`/`renewed_at` — `:724`.

**Persistence (in-memory):** `MemoryWorktreeState { records: HashMap<…>, leases: Vec<WorktreeLease> }`
— `crates/codegg-core/src/worktree_service.rs:311-315`; the lease vector is appended on
acquire (`:426`) and stamped on release (`:488-495`).

**The worktree record itself** is `WorktreeRecord` — `crates/codegg-core/src/worktree_service.rs:117-135` —
carrying `worktree_id`, `project_id`, `repository_id`, `workspace_id`, `node_id`,
`repository_root`, `path`, `branch`, `base_commit`, `managed`, `state`
(`ManagedWorktreeState`, `:33-42`), `health` (`WorktreeHealth`, `:77-84`),
`lease_generation`, `owner_run_id`, `created_at`, `updated_at`.

---

## 3. How is the "lease generation" produced and validated? Monotonic? CAS?

**Produced: a per-worktree monotonically increasing counter, incremented by 1 on each
successful acquire.** Not a timestamp, not a random token, not a UUID.

- In-memory store: `crates/codegg-core/src/worktree_service.rs:415`
  → `record.lease_generation = record.lease_generation.saturating_add(1);`
- SQLite store: `crates/codegg-core/src/worktree_service.rs:642`
  → `let generation = row.get::<i64, _>("lease_generation") as u64 + 1;`
- Initial value is `0` at reservation: `crates/codegg-core/src/worktree_service.rs:870`;
  schema default `DEFAULT 0 CHECK (lease_generation >= 0)` —
  `crates/codegg-core/src/session/schema.rs:2113`.
- The lease-table constraint `CHECK (generation > 0)` —
  `crates/codegg-core/src/session/schema.rs:2124` — means the first lease is generation 1.

**Monotonicity:** strictly increasing per worktree record. It is *not* reset by
`release_lease` (the record's `lease_generation` is untouched at
`crates/codegg-core/src/worktree_service.rs:485-486`); it only advances on the next
acquire. Since the `managed_worktree` row is never deleted, the counter is monotonic for
the life of the record. The in-file test `stale_generation_cannot_release_new_owner`
asserts exactly this (first lease → release → second lease has `generation == 2`) —
`crates/codegg-core/src/worktree_service.rs:1466-1504`, assertion at `:1503`.

**Validated (equality against the record's current counter), in three places:**
- `InMemoryWorktreeStore::renew_lease` — `crates/codegg-core/src/worktree_service.rs:441-446`
- `InMemoryWorktreeStore::release_lease` — `crates/codegg-core/src/worktree_service.rs:475-480`
- `SqliteWorktreeStore::renew_lease` — `crates/codegg-core/src/worktree_service.rs:674-679`
- `SqliteWorktreeStore::release_lease` — `crates/codegg-core/src/worktree_service.rs:712-718`
- Service-level pre-check in `WorktreeService::renew_lease` — `crates/codegg-core/src/worktree_service.rs:961-966`
- `WorktreeService::archive` — `crates/codegg-core/src/worktree_service.rs:1023-1028`
- `WorktreeService::cleanup` (pre-check and post-refresh re-check) —
  `crates/codegg-core/src/worktree_service.rs:1059-1064` and `:1072-1077`

All mismatches return `WorktreeServiceError::StaleGeneration { expected, actual }`
(`crates/codegg-core/src/worktree_service.rs:194-195`).

**Is there a CAS on the generation today? — Partially, and the CAS is NOT on the generation.**

The single-owner CAS in the SQLite store is on `owner_run_id IS NULL`, not on the
generation: `crates/codegg-core/src/worktree_service.rs:644`
```sql
UPDATE managed_worktree
   SET lease_generation = ?, owner_run_id = ?, state = 'in_use', updated_at = ?
 WHERE worktree_id = ? AND owner_run_id IS NULL
```
followed by `if changed.rows_affected() != 1 { return Err(LeaseConflict) }`
(`crates/codegg-core/src/worktree_service.rs:645-647`).

Two consequences C002 must know:

1. **The generation written is computed from a prior read, not compared.** The value at
   `:642` is read at `:628` in the *same* transaction, and the `UPDATE` at `:644` carries
   **no `lease_generation = <observed>` predicate**. A true compare-and-swap on the
   generation does not exist anywhere in the tree.
2. **The acquire transaction is a deferred read-then-write.** `self.pool.begin()` at
   `crates/codegg-core/src/worktree_service.rs:623-627` is a *deferred* transaction
   (no `BEGIN IMMEDIATE`), and the transaction performs a `SELECT` (`:628`) before its
   first `UPDATE` (`:644`). That is the exact shape AGENTS.md describes as the
   `SQLITE_BUSY` upgrade hazard — and the guard that enforces the fix,
   `scripts/check_queue_write_transaction_shape.py`, is scoped to
   `crates/codegg-core/src/session/user_prompt_queue.rs` only
   (`scripts/check_queue_write_transaction_shape.py:39`). **The worktree store is not
   covered by that guard.** `release_lease` has the same deferred read-then-write shape
   (`crates/codegg-core/src/worktree_service.rs:699-703` then `:723-724`).
   *UNPROVEN (outcome, not shape):* whether two concurrent `acquire_lease` calls on one
   worktree produce a clean `LeaseConflict` or a raw storage `SQLITE_BUSY` error under
   WAL. The shape and the guard gap are proven from source; the runtime outcome needs a
   test, which is out of this read-only audit's scope.

---

## 4. WorktreeService operations relevant to a writer permit — exact signatures

All on `impl WorktreeService` (`crates/codegg-core/src/worktree_service.rs:741-1251`):

| Operation | Signature | Lines |
|---|---|---|
| `get` | `pub async fn get(&self, id: &WorktreeId) -> Result<WorktreeRecord, WorktreeServiceError>` | `:787-792` |
| `list` | `pub async fn list(&self, query: WorktreeQuery) -> Result<Vec<WorktreeRecord>, WorktreeServiceError>` | `:794-799` |
| `managed_root` | `pub fn managed_root(&self) -> &Path` | `:783-785` |
| **`reserve`** | `pub async fn reserve(&self, request: &CreateWorktreeRequest) -> Result<WorktreeRecord, WorktreeServiceError>` | `:802-877` |
| **`create`** | `pub async fn create(&self, request: &CreateWorktreeRequest) -> Result<(WorktreeRecord, WorktreeLease), WorktreeServiceError>` | `:880-890` |
| **`create_reserved`** | `pub async fn create_reserved(&self, id: &WorktreeId) -> Result<WorktreeRecord, WorktreeServiceError>` | `:892-942` |
| **`acquire_lease`** | `pub async fn acquire_lease(&self, id: &WorktreeId, owner: &AgentRunId) -> Result<WorktreeLease, WorktreeServiceError>` | `:944-952` |
| **`renew_lease`** | `pub async fn renew_lease(&self, id: &WorktreeId, owner: &AgentRunId, generation: u64) -> Result<WorktreeLease, WorktreeServiceError>` | `:954-968` |
| **`release_lease`** | `pub async fn release_lease(&self, id: &WorktreeId, owner: &AgentRunId, generation: u64) -> Result<WorktreeRecord, WorktreeServiceError>` | `:970-979` |
| **health check** `refresh` | `pub async fn refresh(&self, id: &WorktreeId) -> Result<WorktreeRecord, WorktreeServiceError>` | `:981-995` |
| health write | `pub async fn mark_health(&self, id: &WorktreeId, health: WorktreeHealth) -> Result<WorktreeRecord, WorktreeServiceError>` | `:997-1012` |
| `archive` | `pub async fn archive(&self, id: &WorktreeId, expected_generation: u64) -> Result<WorktreeRecord, WorktreeServiceError>` | `:1017-1044` |
| **`cleanup`** | `pub async fn cleanup(&self, id: &WorktreeId, expected_generation: u64) -> Result<WorktreeRecord, WorktreeServiceError>` | `:1048-1116` |
| `discover` | `pub async fn discover(&self, repository_root: &Path, repository_id: &RepositoryId) -> Result<Vec<DiscoveredWorktree>, WorktreeServiceError>` | `:1118-1150` |
| `reconcile_repository` | `pub async fn reconcile_repository(&self, repository_root: &Path, repository_id: &RepositoryId) -> Result<ReconcileReport, WorktreeServiceError>` | `:1152-1231` |
| `reconcile_all` | `pub async fn reconcile_all(&self) -> Result<ReconcileReport, WorktreeServiceError>` | `:1233-1250` |

Store-level seam (`pub trait WorktreeStore`, `crates/codegg-core/src/worktree_service.rs:272-297`):
`insert_reserved` `:273`, `get` `:274`, `list` `:275-278`, `save` `:279`,
`acquire_lease` `:280-284`, `renew_lease` `:285-290`, `release_lease` `:291-296`.

**What "reserve" actually does:** it reserves an *identity and locator*, not a checkout.
The doc comment is explicit — "Reserve a deterministic identity and locator. No Git
mutation occurs." (`crates/codegg-core/src/worktree_service.rs:801`). It reads Git status
for the base commit (`:812-820`), mints the id and path (`:821-827`), enforces the
outside-the-repository fence (`:832-836`), creates the managed directory (`:837-844`),
verifies no existing worktree collides (`:846-854`), and persists the record in state
`Reserved` with `lease_generation: 0` and `owner_run_id: None`
(`:855-875`).

**`create` is the composed path:** `reserve` → `create_reserved` (does the real
`git worktree add` under the repository lock) → `acquire_lease`
(`crates/codegg-core/src/worktree_service.rs:884-889`).

---

## 5. How does a caller prove it still owns the checkout?

**There is no dedicated "validate lease" / "assert generation" call.** The closest
existing calls are `renew_lease` and `release_lease`, both of which take
`(id, owner, generation)` and both of which *validate* before acting:

- `WorktreeService::renew_lease` — `crates/codegg-core/src/worktree_service.rs:954-968`:
  service-level generation equality check (`:961-966`), then the store re-checks
  generation **and** owner.
  - In-memory: generation check `:441-446`, owner check `:447-449`, and the lease row
    must have `released_at.is_none()` (`:451-459`).
  - SQLite: `UPDATE worktree_lease SET renewed_at = ? WHERE … AND generation = ? AND released_at IS NULL`
    (`:669`); on zero rows affected it distinguishes stale generation from lease
    conflict (`:670-681`).
- `WorktreeService::release_lease` — `crates/codegg-core/src/worktree_service.rs:970-979`:
  same owner+generation pair of checks in both stores (`:475-483`, `:712-721`).
- `WorktreeService::get` — `crates/codegg-core/src/worktree_service.rs:787-792` — returns
  the record, so a caller can compare `record.owner_run_id` and `record.lease_generation`
  itself. This is the only *read-only* ownership proof available.

**Lease expiry: none exists.** There is no expiry field, no deadline, and no sweeper.
`renewed_at` is written (`:460`, `:669`) but never compared against any threshold — the
grep for `expir|deadline|ttl|timeout` in the file returns no matches. Consequently:

- A lease **never expires** and is never automatically released.
- `renew_lease` has **no production caller** anywhere in the tree — grep for `renew_lease`
  matches only `crates/codegg-core/src/worktree_service.rs` (the trait, both impls, and
  the service method) plus an unrelated `renew_leases` field in
  `tests/eggwork_remote_execution.rs:62`. `mark_health` likewise has no production caller
  (only the in-file test at `crates/codegg-core/src/worktree_service.rs:1533`).
- The **only** path that ends a lease other than an explicit `release_lease` is restart
  reconciliation: `reconcile_repository` asks the `WorktreeOwnerResolver` whether the
  owning run is terminal and, if so, calls `store.release_lease(...)`
  (`crates/codegg-core/src/worktree_service.rs:1182-1198`; resolver trait at `:307-309`).
  The resolver is wired in production at `src/core/runtime_deps.rs:376-378`.

**If a lease is released or stolen, what happens to a holder that still believes it owns the checkout?**

Nothing *proactively*. There is no revocation, no broadcast, no event, and no fence the
holder consults. The holder's belief is only falsified **lazily, at its next
`renew_lease`/`release_lease` call**:

- After a release with no successor: the record's `owner_run_id` is `None`
  (`crates/codegg-core/src/worktree_service.rs:485`) and the lease row's `released_at` is
  set (`:493` in memory, `:724` in SQLite). The stale holder's next `renew_lease` finds no
  unreleased lease for that generation and gets `LeaseConflict`
  (`:451-459` in memory; `:669-681` in SQLite, where the generation still matches so the
  `StaleGeneration` branch at `:674-679` is *not* taken and `:680` returns the conflict).
- After a release *followed by a new acquire*: the generation advanced, so the stale
  holder gets `StaleGeneration` (`:441-446` / `:674-679`).
- **A holder that simply never calls renew or release keeps a stale belief indefinitely,
  and no writer path consults the lease at all** — the lease is not consulted by any
  file/Git writer. This is the gap C002's "final-effect authority" call has to close.

---

## 6. Existing lock / CAS / compare-and-swap mechanisms in `worktree_service.rs`

Yes — three distinct mechanisms, none of which is a writer permit.

**(a) In-process, path-keyed async mutex — `WorkspaceLockTable`.**
- Defined in `crates/codegg-core/src/workspace_services.rs:204-246`:
  `inner: DashMap<PathBuf, Arc<AsyncMutex<()>>>` (`:206`), keyed by the *canonical
  repository root* (`:219-245`), returning an RAII `WorkspaceRepositoryGuard` (`:248-251`).
- Acquired inside `worktree_service.rs` at exactly four sites:
  `create_reserved` → `:899`; `acquire_lease` → `:950`; `release_lease` → `:977`;
  `cleanup` → `:1070`.
- **Not** acquired by `reserve` (`:802`), `renew_lease` (`:954`), `refresh` (`:981`),
  `archive` (`:1017`), or `mark_health` (`:997`).
- It is **process-local and per-instance**: every construction site makes its own
  `WorkspaceLockTable::new()` — `src/core/runtime_deps.rs:261`, `:310`, `:374`,
  `src/core/daemon_construct.rs:223`. Two service instances therefore do not exclude
  each other, and the key is a *path*, so a linked worktree at a different path gets a
  different lock (the same aliasing hole C001 records as hole #6).

**(b) Durable single-owner CAS (SQLite).** `SqliteWorktreeStore::acquire_lease` —
`crates/codegg-core/src/worktree_service.rs:618-660`:
deferred `pool.begin()` (`:623-627`) → `SELECT lease_generation, owner_run_id, state`
(`:628`) → owner/state admission checks (`:629-641`) → generation computed (`:642`) →
`UPDATE … WHERE worktree_id = ? AND owner_run_id IS NULL` (`:644`) →
`rows_affected() != 1 ⇒ LeaseConflict` (`:645-647`) → `INSERT INTO worktree_lease`
(`:648`) → `tx.commit()` (`:649-651`).
This is the only durable "one holder" primitive that exists, and it is the shape C002's
"single-owner transactional CAS with monotonic revision" must build on — with the caveats
in §3 (deferred transaction, no generation predicate in the `WHERE` clause).

**(c) Database-level uniqueness.** `idx_worktree_active_lease` — at most one unreleased
lease per worktree (`crates/codegg-core/src/session/schema.rs:2132`), plus
`PRIMARY KEY(worktree_id, generation)` (`:2128`) and the
`UNIQUE(repository_id, path)` / partial `idx_managed_worktree_active_path` constraints on
the record table (`:2117`, `:2133`).

**(d) Lifecycle state machine (in-process guard).** `transition_allowed`
(`crates/codegg-core/src/worktree_service.rs:213-247`) and `transition` (`:249-265`)
constrain `Reserved → Preparing → Ready → InUse → Releasing/Archived/Removed/Orphaned`.
The `Orphaned` state is the existing "uncertain / unsafe" marker
(`:40`, `:1218`).

**(e) A dead constant.** `WORKTREE_MUTATION_EXCLUSIVITY_KEY = "exclusive:worktree-mutation"`
(`crates/codegg-core/src/worktree_service.rs:27`, with the doc comment at `:25-26`
"Scheduler resource key required by worktree/repository metadata mutations. The
scheduler's Git mutation profile already reserves this key.") is **referenced nowhere
else in the tree** — a repo-wide grep for `WORKTREE_MUTATION_EXCLUSIVITY_KEY` matches only
its own definition. There is therefore **no scheduler-level exclusivity over worktree
mutation today**, despite the comment.

---

## 7. How is a worktree's project / repository / workspace identity resolved?

**Inside the worktree service, project/repository/workspace identity is *supplied by the
caller*, never derived.** The service derives only the *path* facts.

- `CreateWorktreeRequest` — `crates/codegg-core/src/worktree_service.rs:148-160` — carries
  `project_id`, `repository_id`, `workspace_id`, `node_id`, `repository_root`,
  `base_commit`, `base_path`, `owner_run_id`.
- `reserve` copies them verbatim into the record —
  `crates/codegg-core/src/worktree_service.rs:856-874` — and `insert_reserved` persists
  them (`:562-563`).
- There is **no verification** inside the service that the supplied `project_id` /
  `repository_id` / `workspace_id` actually correspond to `repository_root`. The service
  does not read the project catalog, the workspace registry, or any binding table.

**The derived part — the repository root:**
`canonical_repo_root` — `crates/codegg-core/src/worktree_service.rs:1289-1332`:
requires a directory (`:1290-1295`), finds the Git root and canonicalizes it
(`:1296-1300`, via `crate::worktree::find_git_root` → `egggit`'s
`crates/egggit/src/worktree.rs:116-122`), and **resolves a linked worktree back to the
common repository root** by reading the `.git` file and walking `…/worktrees/…` to the
common `.git` parent (`:1301-1330`). The result is absolute.

**How the ids themselves are minted:**
- `ProjectId` / `RepositoryId` / `WorktreeId` / `NodeId` / `AgentRunId` — all from the
  `typed_identity!` macro, i.e. UUIDv4 (`crates/codegg-core/src/identity.rs:105-136`,
  declarations at `:217-247`).
- `WorkspaceId` — UUIDv4 minted by the **single producer**, the workspace registry:
  `WorkspaceRegistry::get_or_register` (`crates/codegg-core/src/workspace.rs:480-541`,
  `id: WorkspaceId::new()` at `:527`), keyed by canonical root, deduped under
  `register_lock` (`:501`, `:421`), and persisted in the `workspace` table
  (`crates/codegg-core/src/workspace.rs:203-226`); hydrated at `:427-444` and
  `:461-474`; `resolve` at `:543-554`.
  The type doc states the intent: "It is intentionally not derived from the canonical
  path so that display paths can change (renames, host moves) without renaming
  identifiers" (`crates/codegg-core/src/workspace.rs:30-36`).
  Note `workspace.rs:57-59`: `WorkspaceId::new_unchecked` exists "for compatibility with
  existing internal fixtures and legacy storage hydration" and is used when reading rows
  back (`:239`, `:267`, `:299`).

**The actual binding that ties them together** lives outside the worktree service, in the
daemon catalog table `workspace_project_binding`:
`crates/codegg-core/src/session/schema.rs:1530-1538`
(`workspace_id` PK, `project_id`, `repository_id`, `worktree_id`, `node_id`, `locator`,
`status`, `source`).
The production caller reads it directly:
`src/core/work_order_coordinator.rs:861-886` (SELECT `workspace_id, repository_id` where
`project_id = ? AND status = 'resolved'`), then resolves the workspace
(`:874-876`), finds the Git root (`:877-881`), and parses or mints the `RepositoryId`
(`:882-886`) before building the `CreateWorktreeRequest` (`:894-903`).

**The typed relation type** `ProjectBinding { project_id, repository_id, workspace_id,
worktree_id, node_id }` exists at `crates/codegg-core/src/identity.rs:352-361` with
builders at `:363-388` — but the worktree service does **not** use it.

**What is NOT on the worktree record:** no group id, no principal/actor id, no job id, no
attempt id. The durable *run → worktree* binding lives in the agent-run store instead:
`AgentRunRecord.worktree_id` / `.job_id` / `.attempt_id`
(`crates/codegg-core/src/agent_run.rs:185`, `:187-188`), written by `attach_worktree`
(`:966-988` in-memory, `:1710-1727` SQLite, `:1437-1441` the UPDATE).

---

## 8. The "outside the repository root" native fence at `worktree_service.rs:832-836`

```rust
// crates/codegg-core/src/worktree_service.rs:826-836
let repo_root = self.managed_root.join(request.repository_id.as_str());
let path      = repo_root.join(format!("wt-{id_short}"));
let managed_root = canonical_or_self(&repo_root);
if path.exists() || path.is_symlink() { /* Collision */ }
if managed_root.starts_with(&root) || root.starts_with(&managed_root) {
    return Err(WorktreeServiceError::InvalidInput(
        "managed worktree root must be outside the repository root".into(),
    ));
}
```

**What it does.** It rejects the reservation when the managed worktree root and the
repository root are *mutually* containing — `managed_root` inside `root`, **or** `root`
inside `managed_root` — with `InvalidInput`. `root` is the canonical Git root from
`canonical_repo_root` (`:806`, `:1289-1332`); `managed_root` is
`<self.managed_root>/<repository_id>` canonicalized best-effort.

**Yes — a relative-path fallback defeats it (C001 hole #8 confirmed here).**

The single point of failure is `canonical_or_self`:
`crates/codegg-core/src/worktree_service.rs:1334-1336`
```rust
fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
```
It is **best-effort**: when `canonicalize` fails it returns the path *unchanged*.
`canonicalize` fails when the path does not exist — and at line 832 the managed directory
**has not been created yet** (`create_dir_all` is at `:837`).

So when `self.managed_root` is relative, `managed_root` stays relative at the fence, and
`Path::starts_with` compares component-wise: a relative path never starts with an
absolute root, and an absolute root never starts with a relative path. **Both branches
are false and the fence does not fire.** The directory is then created (`:837`) and
canonicalized to an absolute path (`:842-844`), and the final worktree path is recomputed
from it (`:845`) — so the worktree is materialized wherever the *daemon's process cwd*
points, with the fence already passed.

**Where the relative root comes from (production code, not tests):**
- `src/core/daemon_construct.rs:220-225` —
  `WorktreeService::memory(std::path::PathBuf::from(".codegg/worktrees"), …)`
- `src/core/runtime_deps.rs:259-262` — same relative literal (`CoreRuntimeDeps::new`)
- `src/core/runtime_deps.rs:308-311` — same relative literal (`CoreRuntimeDeps::from_parts`)

The absolute production path is the SQLite constructor:
`src/core/runtime_deps.rs:368-379` uses
`DaemonPaths::default().data_root().join(DEFAULT_WORKTREE_DIRECTORY)`
(`:371-373`; `DEFAULT_WORKTREE_DIRECTORY = "worktrees"` at
`crates/codegg-core/src/worktree_service.rs:28`).

**Preconditions for the defeat** (all three must hold): the relative managed root is in
use; `<managed_root>/<repository_id>` does not yet exist (so `canonicalize` fails); and
the daemon's process cwd is inside the repository root. The first reserve for a given
repository is the vulnerable one; a later reserve finds the directory present, canonicalizes
successfully, and the fence works.

**No guard catches this.** `scripts/check_daemon_cwd_usage.py` matches only
`current_dir()` / `set_current_dir()` call syntax
(`scripts/check_daemon_cwd_usage.py:83-88`, scanned trees `:55-59`, scanned files
`:60-63`) — a relative path *literal* like `PathBuf::from(".codegg/worktrees")` is
invisible to it.

**Secondary effect of the relative root (fail-closed, but permanent):**
`cleanup` calls `ensure_safe_managed_path(&self.managed_root, &record.path)`
(`crates/codegg-core/src/worktree_service.rs:1084`), which re-canonicalizes the root
best-effort (`:1339`) and then requires `path.starts_with(&root)` (`:1340`). With a
relative managed root and an absolute recorded path, that check is false and cleanup
refuses with `"cleanup target is outside the managed root"` (`:1341-1344`).

**A related, independent path-identity gap:** the fence keys on the *repository root
path*, and `canonical_repo_root` deliberately collapses a linked worktree onto the common
root (`:1301-1330`) — but the lock table in §6(a) is keyed by `record.repository_root`
(the common root), while the *file* fence is the only thing keeping the managed tree
outside the checkout. There is no git common-dir mapping consulted by any writer sink
(C001 hole #6).

---

## 9. Does anything today persist a *group* reservation against a worktree?

**No. Nothing.**

- Grep for `group|Group|reservat|Reservat` in `crates/codegg-core/src/worktree_service.rs`
  → **no matches**.
- Grep for `group|reservat` (case-insensitive) in `crates/codegg-core/src/worktree.rs`
  → **no matches**.

The only "reserved" concept in the worktree service is the **lifecycle state**:
`ManagedWorktreeState::Reserved` (`crates/codegg-core/src/worktree_service.rs:33-42`,
string at `:47`), entered by `reserve` (`:868`) and permitted as a lease-acquirable state
(`:405-413` in memory, `:633-641` in SQLite). `insert_reserved` (`:273`, `:561`) persists
it. This is a *creation-phase* state, not a principal-facing reservation.

**Where groups do live:** `crates/codegg-core/src/agent_run_group.rs` —
`AgentRunGroupRecord { group_id: AgentRunGroupId, root_run_id, owner_run_id,
owner: AgentRunGroupOwner, …, idempotency_key }` (`:105-118`), with
`AgentRunGroupService::with_pool` (`:640-642`) and `AgentRunGroupStore` (`:189-193`).
A grep for `worktree|Worktree|reservat` in that file → **no matches**. A group has
**no worktree binding and no checkout reservation** today.

**Therefore:**
- `WorktreeRecord` (`crates/codegg-core/src/worktree_service.rs:117-135`) has **no**
  group column.
- `managed_worktree` (`crates/codegg-core/src/session/schema.rs:2100-2118`) has **no**
  group column.
- `worktree_lease` (`crates/codegg-core/src/session/schema.rs:2120-2131`) has **no**
  group column.
- The C002 plan's precondition — "an explicitly authorized shared reservation" and
  "AgentRunGroup owner" as part of the permit tuple
  (`plans/implementation/agent-workstream-m005-writer-containment-corrective/002-durable-writer-permit-and-quarantine.md:12`, `:16`, `:20`) — **has no durable representation to bind to today**.
- The only durable "reservation-like" table that does exist is
  `workspace_project_binding` (`crates/codegg-core/src/session/schema.rs:1530-1538`),
  which binds *project ↔ workspace ↔ repository ↔ worktree* but carries no group,
  no principal, and no exclusivity.

---

## 10. What storage backend does `WorktreeService` use, and how is it opened?

**`WorktreeService` is backend-agnostic.** It holds `store: Arc<dyn WorktreeStore>`
(`crates/codegg-core/src/worktree_service.rs:734-739`) and there are exactly two
implementations:

| Backend | Type | Lines | Backing |
|---|---|---|---|
| In-memory | `InMemoryWorktreeStore` | `:311-326`, impl `:328-498` | `tokio::sync::Mutex<MemoryWorktreeState>` (`:319`) holding `HashMap<WorktreeId, WorktreeRecord>` + `Vec<WorktreeLease>` (`:311-315`) |
| SQLite | `SqliteWorktreeStore` | `:500-508`, impl `:559-732` | `sqlx::SqlitePool` (`:501`) |

**Production uses SQLite.** `WorktreeService::sqlite(pool, managed_root, locks)` —
`crates/codegg-core/src/worktree_service.rs:767-777` — is wired in the production daemon
constructor at `src/core/runtime_deps.rs:368-379`:
```rust
worktree_service: Some(
    codegg_core::worktree_service::WorktreeService::sqlite(
        pool.clone(),
        codegg_core::storage::DaemonPaths::default().data_root()
            .join(codegg_core::worktree_service::DEFAULT_WORKTREE_DIRECTORY),
        Arc::new(codegg_core::workspace_services::WorkspaceLockTable::new()),
    )
    .with_owner_resolver(Arc::new(DurableWorktreeOwnerResolver { runs: agent_run_store.clone() })),
),
```
The in-memory backend is used by the two convenience constructors
(`src/core/runtime_deps.rs:259-262`, `:308-311`) and by the daemon fallback
(`src/core/daemon_construct.rs:220-225`).

**How the SQLite pool is opened (production bootstrap):**
- `src/main.rs:3948` and `src/main.rs:4120` →
  `storage::init_migrated_daemon_catalog(&catalog_paths)`.
- `crates/codegg-core/src/storage/mod.rs:130-135`: opens a **single-connection** pool
  (`init_pool_at_for_migration`, `:226-238`), runs all migrations
  (`crate::session::schema::migrate`), closes it, then opens the normal pool via
  `init_daemon_catalog` → `init_pool_at` (`:175-218`).
- `connect_and_configure` (`crates/codegg-core/src/storage/mod.rs:240-…`) sets, **on the
  connect options so every pooled connection gets them** (`:251-257`):
  `create_if_missing(true)`, `busy_timeout(30s)`, `foreign_keys(true)`,
  `journal_mode(WAL)`, `synchronous(Normal)`; max 10 connections for the runtime pool
  (`:210`).
- The `managed_worktree` / `worktree_lease` tables are created by **migration v39** —
  `crates/codegg-core/src/session/schema.rs:2097-2145` (tables `:2099-2131`, indexes
  `:2132-2137`).

**Filesystem side:** the managed worktree *checkout* lives on disk at
`<daemon-data>/worktrees/<repository-id>/wt-<short-worktree-id>`
(`crates/codegg-core/src/worktree_service.rs:826-827`, `:845`) — outside the checkout
when the fence holds. Creation and removal are real `git worktree add` / `git worktree
remove` subprocesses, executed inside `spawn_blocking`
(`:900-916` and `:1096-1100`) and delegated to the hardened helpers in
`crates/codegg-core/src/worktree.rs:51-82` (`create_worktree_at`) and `:84-102`
(`remove_worktree`), which build an env-hardened `git` command via `hardened_git_command`
(`crates/codegg-core/src/worktree.rs:115-123`).

---

## Summary of reusable primitives

**Exists and is reusable by C002**

- **`WorktreeId` / `ProjectId` / `RepositoryId` / `NodeId` / `AgentRunId`** — opaque,
  validated, non-path-derived typed identities (`crates/codegg-core/src/identity.rs:105-136`,
  `:217-247`); `WorktreeId` is minted at `worktree_service.rs:821`.
- **`WorktreeId` → deterministic locator** — branch `codegg/worktree/<12 hex>` and
  directory `wt-<12 hex>` (`worktree_service.rs:822-827`), so the permit can key on the
  id and still name a stable path.
- **A durable, generation-fenced ownership record** — `WorktreeLease` (`:137-145`) with
  `owner_run_id` + `generation` + `acquired_at`/`renewed_at`/`released_at`.
- **A monotonic per-worktree generation counter** — incremented by 1 on each acquire
  (`:415`, `:642`), validated by equality in five places (`:441`, `:475`, `:674`, `:712`,
  `:961`), with a `StaleGeneration` typed error (`:194-195`) that already carries
  `expected`/`actual` — exactly the primitive C002's fencing needs.
- **A durable single-owner CAS** — `UPDATE … WHERE worktree_id = ? AND owner_run_id IS NULL`
  + `rows_affected()` check inside a transaction (`:644-647`), plus the DB-level
  `idx_worktree_active_lease` partial unique index
  (`session/schema.rs:2132`) and `PRIMARY KEY(worktree_id, generation)` (`:2128`).
- **A versioned schema with an established migration pattern** — v39 creates the two
  tables and six indexes (`session/schema.rs:2097-2145`); a new permit table is an
  additive v-next migration.
- **Owner+generation validation on every mutating lease action** — renew, release,
  archive, cleanup (`:954`, `:970`, `:1017`, `:1048`).
- **A `Reserved` lifecycle state and a `transition_allowed` state machine** (`:33-42`,
  `:213-247`) including an existing "uncertain" marker, `Orphaned` (`:40`).
- **A restart-reconciliation seam** — `WorktreeOwnerResolver::is_terminal`
  (`:307-309`), used at `:1182-1198`, wired in production at `src/core/runtime_deps.rs:376-378`.
- **A repository-scoped in-process lock** — `WorkspaceLockTable::acquire_repository`
  (`workspace_services.rs:219`), already used by acquire/release/cleanup/create
  (`:899`, `:950`, `:977`, `:1070`).
- **Health inspection** — `refresh` (`:981`), `inspect_health` (`:1253`), `cleanup_safe`
  (`:112`), `WorktreeHealth` (`:77-84`).
- **A durable, typed project↔workspace↔repository binding table** —
  `workspace_project_binding` (`session/schema.rs:1530-1538`), read in production at
  `src/core/work_order_coordinator.rs:861-886`.
- **A durable run→worktree binding** — `AgentRunRecord.worktree_id` + `attach_worktree`
  (`agent_run.rs:185`, `:966-988`, `:1710-1727`).
- **A production SQLite pool with WAL, 30 s busy timeout, foreign keys on**
  (`storage/mod.rs:251-257`).

**Does NOT exist — C002 must build it**

- **No writer permit, no permit state machine (Available/Held/Releasing/Quarantined),
  no quarantine state.** `ManagedWorktreeState` (`:33-42`) has `Orphaned` but nothing
  resembling a permit-level `Quarantined(reason)`.
- **No lease expiry / deadline / TTL and no expiry sweeper** — no matches for
  `expir|deadline|ttl|timeout` in `worktree_service.rs`; `renewed_at` is written but never
  compared to a threshold.
- **No "validate lease / assert I still own it" read-only call** — only the mutating
  `renew_lease` / `release_lease`; `get` exposes raw fields for the caller to compare.
- **No CAS on the generation itself** — the acquire `UPDATE` predicate is
  `owner_run_id IS NULL` (`:644`); the written generation comes from a prior read (`:642`).
- **No group reservation against a worktree** — no group id on `WorktreeRecord`
  (`:117-135`), on `managed_worktree` (`schema.rs:2100-2118`), or on `worktree_lease`
  (`:2120-2131`); `AgentRunGroupRecord` (`agent_run_group.rs:105-118`) has no worktree
  or reservation field either.
- **No actor / principal / job / attempt on the worktree record** — the permit tuple
  required by C002 §7.2 (`c001-restricted-profile-contract.md:246-249`) has nowhere to
  live in the worktree store today.
- **No bounded async waiter / queue for permit acquisition** — `acquire_lease` is
  fail-fast (`LeaseConflict` at `:646`); there is no wait, no notification, no fairness.
- **No idempotent-invocation handling and no monotonic revision column** on the worktree
  record.
- **No scheduler-level exclusivity over worktree mutation** —
  `WORKTREE_MUTATION_EXCLUSIVITY_KEY` (`:27`) is referenced nowhere else in the tree.
- **No projection / event for lease or permit state** — the service returns records and
  leases; it publishes no events.
- **The "outside the repository root" fence is defeatable by a relative managed root**
  (`:832-836` + `canonical_or_self` at `:1334-1336`), and the relative root is live in
  `src/core/daemon_construct.rs:220-225`, `src/core/runtime_deps.rs:259-262`, `:308-311`.
- **The acquire/release transactions are deferred read-then-write** (`:623-627`, `:699-703`)
  and are not covered by `scripts/check_queue_write_transaction_shape.py`, which is scoped
  to `crates/codegg-core/src/session/user_prompt_queue.rs`
  (`scripts/check_queue_write_transaction_shape.py:39`).
- **No `project_id` / `repository_id` / `workspace_id` verification** against
  `repository_root` inside the service — those ids are caller-supplied and unvalidated
  (`:856-874`).

---

### Audit caveats

- This is a **read-only** audit. No file outside this one was created, modified, or
  deleted; no test was executed; no build was run.
- Claims marked with a plain `file:line` are established by reading the cited source in
  the audit checkout. Two claims are explicitly marked **UNPROVEN** (the runtime
  concurrency outcome of two concurrent `acquire_lease` calls under WAL, §3) because they
  require execution, not inspection.
- `crates/codegg-core/src/worktree.rs` contains **no lease, generation, identity, or
  persistence logic at all** — it is a thin `egggit` wrapper for `git worktree
  add/remove/list` plus the hardened Git env policy
  (`crates/codegg-core/src/worktree.rs:31-135`). Every lease fact in this document comes
  from `worktree_service.rs`.
