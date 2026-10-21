# Managed Agent Scratch Milestone M007 — Scratch retention sweep and unreclaimable-tree disposition

Status: ready for handoff
Repository baseline: `17b0b106` on `main`

Source roadmap: `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`
Source predecessor: `plans/implementation/managed-agent-scratch/006-scratch-usage-census-and-operator-visibility.md`
Source predecessor closure: none — M006 is registered `ready`, not implemented (see §2)
Long-term requirements: `plans/000-long-term-specification.md` (agent operability and
authority), `plans/002-long-term-roadmap.md`
Applicable ADRs: None required. This milestone **reclaims** scratch, but it changes
no ownership boundary, grants no access, and widens no authority: it removes trees
the daemon already owns, under a policy the operator configures, on a lease derived
from durable state that already exists. If the implementing agent finds that any
part of this plan requires a change to *who may do what* — in particular a change
to the sandbox grant set — it must stop and propose an ADR instead (see §14).
Primary class: capability (bounded reclamation of the per-user scratch tree), with
one invariant component (Unix/Windows reclaim-validator parity, §3.3) and one
operator surface (an explicitly audited reclaim action, §3.6).

## 1. Objective

**Bound the growth of the per-user scratch tree.** Today the tree grows without
limit: three event-driven reclaimers exist and a session that is closed but never
soft-deleted keeps its tree forever. Worse, on Linux and macOS a tree that
contains one multiply-linked file can **never** be reclaimed at all — not by the
automatic path, not by a retry, not after a restart — so a single `ln` inside a
model's own scratch permanently pins that session's whole tree.

M007 delivers five things:

1. a **retention policy** — an age-based, configurable rule with a default of 14
   days, expressed in durable terms so deciding it needs no filesystem walk;
2. a **daemon-owned sweep** that reclaims idle trees past the retention age, and
   reclaims **orphan** trees whose owning session no longer exists;
3. a **liveness lease** derived from durable state that already exists, closing
   the race against a live turn's `TMPDIR` — **no new lease primitive**;
4. the **Unix/Windows reclaim-validator parity fix** that removes the permanent
   pin on Linux and macOS;
5. an **explicit, audited operator action** for a tree the automatic path still
   refuses.

The milestone is bounded: it reclaims scratch the daemon already owns, and it
reclaims nothing else.

## 2. Why this milestone is ready

Hard dependencies, all closed:

- **M001** (`plans/closure/managed-agent-scratch/001-status.md`) — the scratch
  layout, the owner record, and the session-tree identity rules.
- **M002** (`plans/closure/managed-agent-scratch/002-status.md`) — the delegated
  run-private and shared resource roots, and the attempt leases.
- **M004** (`plans/closure/managed-agent-scratch/004-status.md`) — the
  reclamation primitives this milestone drives: the owner-validated tombstone
  path, the crash-resumable tombstone recovery, and the soft-deleted-session
  reclaim.
- **M005** (`plans/closure/managed-agent-scratch/005-status.md`) — closed, with
  the live observation recorded. The scratch root this milestone reclaims is
  administered and real.
- **C001** (`plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md`)
  — the Linux native containment qualification is closed, so the trees this
  milestone reclaims are trees whose grants are already evidence-qualified on
  Linux.

Interface dependencies, stable:

- The daemon socket request/response surface
  (`crates/codegg-protocol/src/core.rs`) accepts an additive request variant.
- The CLI lifecycle-command surface (`enum DaemonCommand`, `src/main.rs:653`)
  accepts an additive subcommand.
- The durable session, agent-run, and job stores are unchanged in shape; the
  lease predicate reads them, it does not extend them.
- The tombstone reclaim path (`src/scratch.rs`) is unchanged in shape; this
  milestone removes one refusal from it (§3.3) and calls it from a new place.

**One open input, named and bounded: M006's census.** The retention policy's
*shape* is specified in this plan. Its *default values* are the
contract-derived starting point in §6.1 — chosen from what the disposal
contract permits, not from a measurement — and the measurement that validates
them is M006's census. M006 is registered `ready` and carries no open decision,
so **WP0** below lands and runs it before the policy is implemented. Until that
census has run, no closure may claim the defaults are *measured*; it may claim
only that they are *specified* and that the census was run.

This is a deliberate departure from the roadmap's M006 exit text, which
deferred registering this milestone "until the census has produced real
numbers". The operator has directed that the milestone be written and
registered now. The consequence is handled by construction rather than by
hope: the numbers enter as a named work package with a named fallback, not as
an unstated assumption baked into a threshold.

## 3. Current implementation evidence

### 3.1 Reclamation is event-driven, and three events is all there is

`src/scratch.rs` owns the tree. A session's tree is
`<cache>/codegg/scratch/sessions/<hex(sha256(session_id))>`
(`src/scratch.rs:108-145`); the digest is computed at `src/scratch.rs:136-140`
and the owner record — which stores the **digest**, not the session id — is
written at `src/scratch.rs:143` (`ensure_owner_record`,
`src/scratch.rs:481-540`). A delegated run's private root and a shared
resource's root are children of that tree.

Exactly three things remove a tree today:

- a **revoked shared resource** (`reclaim_shared_scratch_dir`,
  `src/scratch.rs:177-194`);
- a **terminal delegated run** (`reclaim_agent_run_scratch_dir`,
  `src/scratch.rs:200-222`);
- a **soft-deleted session** (`reclaim_deleted_session_scratch_dir`,
  `src/scratch.rs:228-241`).

There is no sweep, no TTL, and no size ceiling. A session that is closed but
never soft-deleted keeps its tree indefinitely. This is stated in the module
contract (`architecture/security.md:310-316`, `docs/tools.md:313-318`).

### 3.2 The reclaim path is sound, and it is the right path to reuse

`reclaim_owned_resource_dir` (`src/scratch.rs:276-293`) atomically renames the
managed directory to an opaque tombstone, then validates and removes it.
`validate_and_remove_tombstone` (`src/scratch.rs:341-381`) checks the tombstone
is a real directory under the canonical parent, checks the owner record is a
regular, owner-private file whose content matches the digest, then validates
the tree and removes it. `recover_reclaim_tombstones`
(`src/scratch.rs:306-338`) re-arms an orphaned tombstone after a daemon
interruption, and concurrent reclamation is serialized by a process-wide lock
(`src/scratch.rs:12`, `:181-184`).

M007 must **reuse** this path, not reimplement it. A second, weaker removal
routine would be a new way to delete the wrong thing.

### 3.3 The Unix multiply-linked refusal is a defect, and the code already says why

`validate_reclaim_tree` (`src/scratch.rs:384-411`) refuses three things: a
symlink or reparse point (`:386-388`), a non-regular file (`:395-397`), and — on
Unix only — **a regular file whose link count is not 1** (`:398-404`).

The comment immediately below that check, at `src/scratch.rs:405-410`, states:

> A hard link is a file name, not a recursive edge. Removing the name under the
> claimed scratch tombstone only decrements its link count; it does not modify
> or remove the outside file. Windows does not expose a stable link-count
> accessor at this crate's MSRV, so quarantine reparse points and let the
> platform's handle-based remove_dir_all unlink names.

That comment was added by commit `5905a2bb` ("fix(scratch): reclaim revoked
resources safely on Windows") — the same commit that made the check
`#[cfg(unix)]`-only. It documents the reasoning that makes the check
**unnecessary**, and it describes the Windows behaviour as the correct one. The
Unix check was left behind, and it is now the only platform that refuses a
multiply-linked file.

The repository already contains the proof on both sides:

- The **Unix** test (`src/scratch.rs:797-851`, hardlink half at `:832-850`)
  hardlinks an outside file into a scratch tree and asserts the reclaim
  **errors** — then asserts, in the same test, that the outside file's content
  is `"outside content"`, i.e. it is already asserting the safety property that
  the refusal does not provide.
- The **Windows** test (`src/scratch.rs:853+`) hardlinks an outside file into a
  scratch tree and asserts the reclaim **succeeds** and the outside file's
  content survives.

Unlinking a directory entry cannot modify or remove the file that entry names;
it decrements a link count. `std::fs::remove_dir_all` — the routine the reclaim
path actually calls, at `src/scratch.rs:379` — does not follow symbolic links,
on either platform. So the Unix refusal protects nothing that the removal does
not already protect, and it costs the tree permanently.

**What the refusal costs.** The reclaim path renames the tree to a tombstone
*before* validating it (`src/scratch.rs:286`). When validation then refuses,
the tree is already gone from its managed name and the tombstone is never
removed: `recover_reclaim_tombstones` (`:306-338`) finds it, renames it to a
freshly claimed name, validates it, fails the same way, and returns the error.
The tombstone is retried forever and never reclaimed. The session's managed
name is free for a new generation, so the *directory* stops growing — but the
*bytes* of the refused tree are pinned for the life of the install, and every
retry re-walks it.

**That refusal is reachable under the strongest containment.** The Landlock
backend requests `write_access = AccessFs::from_all(abi)` for every writable
root (`src/security/sandbox/landlock.rs:80`, applied to the write roots at
`src/security/sandbox/landlock.rs:113-115`), and `from_all` includes **REFER** —
the right that governs linking. ABI 3 is the minimum contract precisely because
"v2 adds REFER" (`src/security/sandbox/landlock.rs:75`). A hard link whose
source and destination are in the *same* directory therefore needs REFER only on
that one directory, which a writable root has. The Linux native qualification
(C001) proves only the **cross-boundary** case is denied; the same-directory
case is permitted. macOS Seatbelt re-allows `file-write*` for the writable roots
(`src/security/sandbox/seatbelt.rs:104-109`), so the same is permitted there.
Windows is immune, because it has no stable link-count accessor at this crate's
MSRV and therefore never made the check.

So: on Linux and macOS, one `ln a b` inside a model's own scratch permanently
pins that session's whole tree, under Landlock Required, with no human action
able to release it.

### 3.4 The liveness predicate already exists, in the shape M007 needs

`reclaim_deleted_session_scratch_when_idle` (`src/core/daemon_sessions.rs:1202-1254`)
is the proven pattern:

- it takes SQLite's **writer lock** and re-reads the row inside that lock
  (`:1211-1222`), so a concurrent restore either wins first (0 rows) or waits
  until the cleanup commits;
- it refuses to reclaim while any **agent run** for the session is non-terminal
  (`:1223-1230`);
- it refuses while any **job** for the session is non-terminal — the states
  `Scheduled`, `Queued`, `Running`, `Interrupted`, `Blocked`
  (`:1231-1249`);
- it then reclaims (`:1250`) and commits (`:1252`).

M007 needs the same predicate for a *different* question (idle-by-age, not
deleted). It reuses the same three durable signals and adds one more that
already exists:

- a **`turn_admission_receipt`** row in status `accepted` for the session. The
  receipt's statuses are `accepted | completed | failed | interrupted`
  (`crates/codegg-core/src/session/schema.rs:3555-3583`); a turn is admitted with
  `accepted` and reaches a terminal status through `finish_turn_receipt`
  (`crates/codegg-core/src/session_control.rs:727-743`). A receipt still
  `accepted` therefore means *a turn was admitted and has not finished*. The
  daemon already uses exactly this signal to mark interrupted turns at startup
  (`interrupt_incomplete_turn_receipts`,
  `crates/codegg-core/src/session_control.rs:746-759`), which is what makes it
  trustworthy after a restart: a receipt left `accepted` by a crashed daemon is
  marked `interrupted` during bootstrap
  (`src/core/daemon_bootstrap.rs:206-211`), so it cannot masquerade as live work
  forever.

**No new lease primitive, no new table, no migration.** This is the decisive
reason M007 can be one pass: the liveness question is already answerable from
durable state.

### 3.5 The durable↔disk mapping is one-way, so the sweep must map it in reverse

The tree name is `hex(sha256(session_id))` — a one-way digest. The owner record
stores the **digest**, not the session id (`src/scratch.rs:143`, `:481-540`).
Nothing on disk can be inverted back to a session id, by design.

So the sweep cannot start from the disk and ask "who owns this?". It must start
from the durable side: enumerate the durable session ids, hash each with the
same function `session_scratch_dir` uses, and build the digest set. A tree whose
digest is in that set is owned; a tree whose digest is not is an **orphan**.

Two facts make this safe:

- The digest computation is currently written out **twice** — once in
  `session_scratch_dir` (`src/scratch.rs:136-140`) and once in
  `existing_session_scratch_dir` (`src/scratch.rs:247`). A third divergent copy
  in the sweep would silently break the mapping. M007 must factor the
  computation into one named function used by all three.
- Hashing every durable session id is cheap. The sweep needs no size traversal
  to decide anything, which keeps the roadmap's "no size traversal on agent hot
  path" and "no global scan per tool call" constraints satisfied by
  construction — the sweep runs on its own cadence, and the *decision* is a
  durable timestamp.

### 3.6 There is no operator action for a tree the automatic path refuses

The daemon CLI exposes `Start`, `Stop`, `Status`, `Logs`, `Attach`
(`src/main.rs:653-690`); `Status` reaches the daemon through
`SocketCoreClient::connect` (`src/main.rs:1663-1668`). Nothing can ask the
daemon to dispose of one named tree.

Every `CoreRequest` variant is classified by `operation_descriptor`
(`crates/codegg-core/src/authorization/policy.rs:101`) and rendered into the
operation-to-capability matrix (`operation_capability_matrix`,
`crates/codegg-core/src/authorization/policy.rs:1435`; documented in
`architecture/authorization.md`). `scripts/check_authorization_matrix.py`
fails if a variant is unclassified, and fails if the descriptor table grows a
wildcard arm. Every audit action must have a coverage row
(`crates/codegg-core/src/audit_instrumentation.rs:91`), and every operation must
be either live-mapped (`INSTRUMENTED_OPERATIONS`,
`crates/codegg-core/src/audit_instrumentation.rs:545`) or explicitly listed as
uninstrumented; `scripts/check_audit_coverage.py` enforces both.

A destructive operator action therefore needs all of: a protocol variant, a
descriptor arm, a matrix row, an audit action, a coverage row, an
instrumentation mapping, and a CLI surface.

### 3.7 The socket server drops an undecodable frame silently

The daemon socket decodes each frame with
`if let Ok(frame) = serde_json::from_str::<CoreFrame>(trimmed)`
(`src/core/transport/daemon_socket.rs:445`) and has **no `else` branch**. A
client that sends a request an older daemon does not know receives **no
response at all**. The operator command must therefore use a **bounded** client
timeout and report a typed "this daemon does not support scratch reclaim;
restart it to upgrade" error. Hanging is not an acceptable degradation.

### 3.8 The write path has a known lock-upgrade hazard

A queue write path that reads before it writes must open its transaction with
`begin_immediate` (`BEGIN IMMEDIATE`), not `pool.begin()`. A deferred
transaction takes no lock, so the first `SELECT` holds a shared read lock and
the first `UPDATE` has to upgrade it — and SQLite refuses that upgrade
**without waiting**, so two such callers can both fail with `SQLITE_BUSY`
instead of one winning and the other waiting out `busy_timeout`. The guard is
`scripts/check_queue_write_transaction_shape.py`, currently scoped to one file
(`crates/codegg-core/src/session/user_prompt_queue.rs:39`), where the private
`begin_immediate` helper lives (`:1081`).

M007's sweep **reads before it writes** — it reads the session's activity and
liveness, then reclaims — so it is exactly this hazard. The helper must be
promoted to a shared, reusable location and the guard extended to the new
sweep module (§7 WP3).

### 3.9 The disposable-cache contract, which this milestone must preserve

"Cache contents are disposable and may be removed by host cache maintenance"
(`architecture/process-tool-execution-ownership.md:39-41`,
`architecture/security.md:295-297`). M007 is added **around** that sentence; it
does not narrow or widen what host cache maintenance may do, and it does not
make scratch durable.

## 4. Invariants that must not regress

- **Scratch stays disposable.** Nothing this milestone does makes a scratch tree
  durable, authoritative, or a source of results. Transcripts, durable run
  results, and managed Git worktrees remain outside the scratch tree.
- **The reclaim path stays the reclaim path.** Every removal goes through the
  existing owner-validated tombstone path (`reclaim_owned_resource_dir` →
  `validate_and_remove_tombstone` → `validate_reclaim_tree`). A second removal
  routine is out of scope and is a defect if introduced.
- **Ownership checks are never relaxed.** The owner record must match the
  digest; the resolved path must be inside the canonical sessions root; a
  symlinked component is refused. The parity fix in §3.3 relaxes an *entry
  content* check, never an *ownership* or *containment* check.
- **A live turn never loses its `TMPDIR`.** The sweep must not reclaim a tree
  whose session has in-flight work (§3.4). A turn that starts *after* the sweep
  commits gets a freshly created scratch root, which is the same behaviour the
  existing soft-delete path already has.
- **No size traversal on the agent hot path.** No tool call, permission check,
  turn admission, or scheduler decision may walk the tree. The sweep's decision
  uses durable timestamps only.
- **One digest computation.** The tree name remains `hex(sha256(session_id))`,
  and the run/shared names remain `hex(sha256("<scope>:<id>"))`. The sweep
  reuses the existing computation; a divergent copy would silently break the
  durable↔disk mapping.
- **The one-way digest stays one-way.** The sweep maps durable session ids
  forward into digests. It must not attempt to invert a digest, and it must
  not persist a digest→session-id mapping that does not already exist.
- **The sweep never follows a link and never reads content.** Only
  `symlink_metadata` and directory listing are used.
- **Daemon-bound code does not consult the process CWD**
  (`scripts/check_daemon_cwd_usage.py` scans `src/core`).
- **`codegg-core` stays free of UI, server, plugin, and auth concerns**
  (`scripts/check-core-boundary.sh`). The filesystem walk and the removal stay
  in `src/scratch.rs`; the durable reads and the transaction helper stay in
  `codegg-core`; the orchestration stays in `src/core`.
- **The operator action is local-only and audited.** The reclaim request is
  served on the local daemon socket. It is not exposed on the remote HTTP/WS
  surface.
- **The reclaim request is additive.** `PROTOCOL_VERSION` stays `2` unless the
  implementation cannot keep the request decodable by an older peer; the default
  is to keep it at 2 and degrade gracefully (§9).

## 5. Scope

### In scope

- **The Unix/Windows reclaim-validator parity fix**: remove the Unix
  `nlink() != 1` refusal in `validate_reclaim_tree`, keeping the symlink and
  non-regular refusals. Align the Unix test with the Windows test's assertion.
- **The retention policy**: a `scratch` configuration section with `enabled`,
  `retention_days`, `sweep_interval_minutes`, and `orphan_grace_hours`; a
  daemon-owned sweep task; a durable liveness predicate; soft-deleted-session,
  idle-by-age, and orphan reclamation.
- **The operator action**: `codegg daemon scratch-reclaim --tree <digest>
  [--apply]`, served over the local daemon socket, audited, dry-run by default.
- **The authorization and audit surfaces** that the operator action requires: a
  `CoreRequest`/`CoreResponse` pair, a descriptor arm, a matrix row, an audit
  action, a coverage row, and an instrumentation mapping.
- **Documentation**: `architecture/security.md`, `architecture/authorization.md`,
  `architecture/audit.md`, `architecture/process-tool-execution-ownership.md`,
  `docs/tools.md`, `docs/daemon.md`, `codegg.example.jsonc`, and the
  `tool-execution` skill.
- **Focused unit and integration tests**, including the negative cases (a
  multiply-linked file, a planted symlink, a live turn's tree, a
  restore-during-sweep race, a vanished tree, a budget-truncated sweep).

### Explicitly out of scope

- **Changing the scratch layout, the identity rules, or the owner-record
  format.**
- **Changing the symlink or non-regular-file refusals.** The quarantine of a
  tree containing a planted symlink stays. Only the multiply-linked refusal
  goes; the operator action in §3.6 is the disposition for what remains.
- **Any new lease table, lease column, or migration.** The lease is derived
  from durable state that already exists (§3.4).
- **Any byte-based eviction.** M007's criterion is age. A byte ceiling is a
  *reporting* dimension (M006's census), not an eviction trigger; evicting by
  size would need the same lease and adds a second, harder-to-size criterion.
- **Publishing into `src/util/metrics.rs`.** That registry has no consumer
  anywhere in the workspace; a gauge with no reader is an unobservable claim.
- **Windows native sandbox qualification**, which the operator has explicitly
  deferred to a separate workstream. M007's parity fix *removes* a Unix-only
  divergence, so it narrows the platform difference; it claims no Windows
  containment and changes no Windows behaviour.
- **Changing what a model, a tool, or a human may do.** M007 reclaims the
  daemon's own disposable cache.
- **Touching the user's own daemon.** Live observation, if performed, uses an
  isolated daemon home exactly as M005 did.

## 6. Required production changes

### 6.1 Configuration and defaults

A new optional `scratch` configuration section, with these **defaults**:

| Key | Default | Meaning and floor |
|---|---|---|
| `enabled` | `true` | Whether the daemon-owned sweep runs at all. Off means the sweep never fires; the existing event-driven reclaimers are unaffected either way. |
| `retention_days` | `14` | Age, in days, past which an **idle** session's scratch tree is reclaimable. A value below `1` is rejected or clamped — `0` would mean "reclaim every idle tree immediately", which is not a policy anyone can intend. |
| `sweep_interval_minutes` | `360` | How often the sweep runs. A value below `5` is rejected or clamped, so a mistyped value cannot become a busy loop. |
| `orphan_grace_hours` | `24` | How long a tree whose digest matches no durable session is left alone before it is reclaimed as an orphan. The grace period absorbs the window in which a tree exists before its durable session row is visible to the sweeper. |

**These defaults are specified, not measured.** They are derived from the
disposal contract — scratch is disposable cache data, and 14 days is well beyond
any plausible live session — not from a census. WP0 (§7) is what turns them
into a measured value, and §15 requires the closure record to say which of the
two it is.

### 6.2 Core/domain

- A named digest function in `src/scratch.rs` — `session_scratch_digest(
  session_id) -> String` — used by `session_scratch_dir`,
  `existing_session_scratch_dir`, and the sweep. One computation, three callers.
- A reclaimability verdict shared with `validate_reclaim_tree` rather than
  restated, so a report cannot drift from the behaviour it describes (the same
  rule M006 is required to share).

### 6.3 Storage and migrations

- **No schema change, no `ALTER TABLE`, no `STORAGE_LAYOUT_VERSION` bump.**
  `scripts/check_project_catalog_invariants.py` is therefore not triggered.
- The lease reads the existing `session`, `turn_admission_receipt`, agent-run,
  and job tables.
- `begin_immediate` is promoted from the private helper at
  `crates/codegg-core/src/session/user_prompt_queue.rs:1081` to a shared,
  reusable helper, and `user_prompt_queue.rs` is refactored onto it. The
  promotion must not change the queue store's existing transaction shapes —
  `scripts/check_queue_write_transaction_shape.py --self-test` must still pass.

### 6.4 Protocol and DTOs

- One additive `CoreRequest` variant carrying the reclaim request and one
  additive `CoreResponse` variant carrying the result.
- The request names a tree by its **opaque digest** only. It carries no path,
  no session id, and no filesystem path of any kind: the daemon resolves the
  digest to the single tree it owns.
- The response reports the tree's digest, the entry count, the byte count, the
  verdict, the refusal reason if any, and whether anything was removed. It
  carries no absolute host path and no file content.
- `PROTOCOL_VERSION` stays `2` (§9).

### 6.5 Runtime and concurrency

- A daemon-owned sweep task, spawned where the other maintenance task is
  spawned (`src/core/daemon_construct.rs:281-288` holds the 300 s projection
  maintenance tick), on its **own** interval and its **own** handle. It is not
  the projection tick's business: that owner is the projection retention
  service, and ADR-0039 already rejected coupling unrelated concerns to it.
- The sweep holds no request path hostage: it is a spawned task, it is
  cancellable on shutdown, and it never blocks a tool call, a permission check,
  a turn admission, or a scheduler decision.
- The sweep is bounded per pass: a maximum number of candidate trees examined,
  a maximum number of trees reclaimed, and a wall-clock budget. A pass that
  hits its budget stops and records that it stopped.
- The sweep's per-candidate work runs in an **immediate** transaction (§3.8):
  take the writer lock, re-read the session's activity and liveness inside that
  lock, reclaim only if the recheck still says the tree is reclaimable, then
  commit. This is the M004 pattern generalized from "deleted" to "idle by age".
- The reclaim path's existing process-wide serialization
  (`src/scratch.rs:12`) is reused; the sweep does not add a second lock for
  the same tree.

### 6.6 Frontend or operator surface

- `codegg daemon scratch-reclaim --tree <digest>` — **dry run by default**:
  reports what would be removed and removes nothing. `--apply` performs the
  removal. `--endpoint` matches the other lifecycle commands.
- The client applies a **bounded** timeout (§3.7).
- The command is served on the **local daemon socket only**.

### 6.7 Security and authorization

- A new operation `scratch_reclaim` in `operation_descriptor`
  (`crates/codegg-core/src/authorization/policy.rs:101`), scoped `Opaque` — no
  project linkage is available, so team principals fail closed and only the
  local-owner broad policy authorizes — and gated on `Capability::FileModify`
  (`crates/codegg-core/src/team.rs:248-295`), the closest existing capability
  for a destructive local filesystem action.
- A row in `architecture/authorization.md`'s operation-to-capability matrix.
- A new audit action `scratch_reclaim` (`crates/codegg-core/src/audit.rs:101-131`,
  with `AuditAction::ALL`'s fixed length updated from 27), a coverage row in
  `REQUIRED_AUDIT_COVERAGE`
  (`crates/codegg-core/src/audit_instrumentation.rs:91`), and a mapping in
  `INSTRUMENTED_OPERATIONS` (`crates/codegg-core/src/audit_instrumentation.rs:545`).
- The audit metadata is **bounded**: the opaque digest, the entry count, the
  byte count, the verdict, the refusal reason, and the outcome. No absolute
  path, no file name outside the tree, no content, no session id.
- The operator removal relaxes the **entry-content** refusals only (a planted
  symlink or a non-regular entry is removed by name, not refused). It never
  relaxes the ownership check, the containment check, or the owner-record
  check. Its safety argument is the link-safety of `std::fs::remove_dir_all`,
  which does not follow symbolic links on either platform — and that argument
  is only accepted if the test in §10 proves it.

## 7. Ordered work packages

### WP0 — The census input

Intent: obtain the measurement M006 exists to produce, before a threshold is
implemented.

Required changes: land M006
(`plans/implementation/managed-agent-scratch/006-scratch-usage-census-and-operator-visibility.md`)
if it is not already landed, then run its census against a real scratch tree and
record the observed age distribution, byte distribution, tree count, and the
count of trees the reclaim validator refuses.

Acceptance evidence: the census output, recorded in M007's closure record, with
the environment it was taken in named.

**If the census contradicts §6.1's defaults**, the implementing agent MUST
adjust the defaults before implementing them and MUST record the adjustment and
its reason in the closure record. The defaults are a starting point, not a
claim. If the census cannot be obtained, the agent proceeds with §6.1's
defaults and MUST say so plainly in the closure record — an unobtained
measurement is recorded as unobtained, never as a passing one.

### WP1 — Reclaim-validator parity (Unix)

Intent: remove the permanent pin a multiply-linked file causes on Linux and
macOS.

Required changes: remove the `#[cfg(unix)]` `nlink() != 1` refusal from
`validate_reclaim_tree` (`src/scratch.rs:398-404`), keeping the symlink and
non-regular refusals. Update the Unix test at `src/scratch.rs:797-851` so the
hardlink half asserts what the Windows test already asserts: the reclaim
**succeeds** and the outside file's content survives. Keep a test that the
symlink half still refuses.

Acceptance evidence: the Unix test proves (a) a tree containing a hardlink to
an outside file is reclaimed, (b) the outside file's content is unchanged
afterwards, and (c) a tree containing a planted symlink is still refused and
quarantined.

**This is the highest-risk change in the milestone.** The safety argument is
that unlinking a directory entry decrements a link count and never modifies the
file it names, and that `remove_dir_all` does not follow links. That argument
must be demonstrated by the test, not asserted in a comment.

### WP2 — The configuration surface

Intent: make the policy operator-visible and adjustable.

Required changes: a `ScratchConfig` in `crates/codegg-config/src/schema.rs` with
`enabled`, `retention_days`, `sweep_interval_minutes`, and `orphan_grace_hours`;
a `scratch` field on `Config` (`crates/codegg-config/src/schema.rs:218`); a
merge arm in `merge_configs` (`crates/codegg-config/src/paths.rs`) and a
`ScratchConfig::merge` following the `DaemonConfig` pattern
(`crates/codegg-config/src/schema.rs:2071-2096`); the section documented in
`codegg.example.jsonc`.

Acceptance evidence: `scripts/check_config_merge_coverage.py` passes;
`codegg validate --config codegg.example.jsonc` passes; a test asserts the
layer-merge behaviour of the new section (a later layer's `retention_days`
wins; an absent field inherits).

### WP3 — The durable liveness predicate and the sweep

Intent: reclaim idle trees past the retention age, and reclaim orphans.

Required changes: a new module `src/core/scratch_sweep.rs` that

- enumerates the durable sessions (including soft-deleted rows) and hashes each
  id with the shared digest function to build the owned-digest set;
- enumerates the session trees under the scratch root and partitions them into
  owned and orphan by digest;
- for each owned tree, computes the candidate age from durable state (§6.1) and
  applies the lease predicate (§3.4) inside an immediate transaction;
- for each orphan tree, applies the orphan grace period (§6.1);
- reclaims through the existing tombstone path and records what it did.

The sweep must recheck inside the writer lock, exactly as
`reclaim_deleted_session_scratch_when_idle` does
(`src/core/daemon_sessions.rs:1211-1252`).

**Transaction shape**: the sweep reads before it writes, so the transaction
opens with `begin_immediate`. Promote the private helper at
`crates/codegg-core/src/session/user_prompt_queue.rs:1081` into a shared helper
and extend `scripts/check_queue_write_transaction_shape.py` (currently scoped to
one file at `:39`) to cover the new sweep module. The extension must keep the
guard's existing semantics and must still fail on its `--self-test` injection.

Acceptance evidence: a test shows a session with an `accepted` turn receipt is
**not** swept; a test shows a session with a non-terminal run is not swept; a
test shows a session with a non-terminal job is not swept; a test shows a
soft-deleted session's tree is swept; a test shows an idle, old session's tree
is swept; a test shows a fresh session's tree is not swept; a test shows an
orphan tree is swept only after the grace period; a test shows the sweep is
bounded and reports that it stopped at its budget.

### WP4 — The protocol variant and the authorization arm

Intent: let a local operator ask the daemon to dispose of one named tree.

Required changes: the `CoreRequest`/`CoreResponse` variants and their DTOs in
`crates/codegg-protocol`; the dispatch arm in `src/core/daemon.rs`; the
descriptor arm in `crates/codegg-core/src/authorization/policy.rs:101`; a
representative request in `representative_requests()` so the row appears in
`operation_capability_matrix` (`:1435`); the row in
`architecture/authorization.md`.

Acceptance evidence: a socket round-trip test returns the result; a test shows
the request cannot name a path or a session id; a test shows the operation is
refused for a principal that is not the local owner; a test shows the operation
is not reachable on the remote HTTP/WS surface.

### WP5 — The audit action

Intent: leave a durable record of an operator-initiated removal.

Required changes: the `AuditAction::ScratchReclaim` variant and its `ALL` entry
and `as_str` arm (`crates/codegg-core/src/audit.rs:101-163`); the coverage row
in `REQUIRED_AUDIT_COVERAGE`
(`crates/codegg-core/src/audit_instrumentation.rs:91`); the mapping in
`INSTRUMENTED_OPERATIONS` (`:545`); the taxonomy row and the operator matrix in
`architecture/audit.md`.

Acceptance evidence: `scripts/check_audit_coverage.py` passes; a test shows the
audit record is emitted for an applied reclaim and for a refused one, with
bounded metadata and no absolute path.

### WP6 — The operator command

Intent: make the disposition reachable.

Required changes: the `codegg daemon scratch-reclaim` subcommand in
`src/main.rs` (modelled on `DaemonCommand::Status`, `src/main.rs:1663-1668`), its
dry-run and `--apply` forms, and a **bounded** client timeout with a typed
error for a daemon that predates the request (§3.7).

Acceptance evidence: a CLI test against a live daemon prints the dry-run
report and removes nothing; the `--apply` form removes the tree and reports it;
a client against a daemon without the request fails with the bounded error
rather than hanging.

### WP7 — Documentation and planning reconciliation

Intent: keep the contract and the planning surface truthful.

Required changes: `architecture/security.md` (the retention policy, the sweep,
the lease, and the corrected Unix/Windows paragraph — the doc currently states
"Unix quarantines symlinks, non-regular entries, and multiply-linked regular
files", which §3.3 shows is no longer true after WP1);
`architecture/process-tool-execution-ownership.md` (the disposable-cache wording
preserved verbatim); `docs/tools.md`; `docs/daemon.md` (the lifecycle-command
table); `architecture/authorization.md`; `architecture/audit.md`;
`codegg.example.jsonc`; and the `tool-execution` skill, which must agree with
its architecture doc.

Acceptance evidence: `codegg validate --config codegg.example.jsonc` passes; no
document claims a capability the milestone does not deliver; the skill and the
architecture doc say the same thing about the sweep.

## 8. Failure, cancellation, restart, contention semantics

- **A failed sweep pass** must not leave the daemon in a bad state. The next
  pass retries; a tree that cannot be reclaimed is left where it is and
  recorded. A pass never removes a tree it did not verify.
- **A cancelled sweep** (daemon shutdown) leaves the tree as it was. The
  tombstone path is already crash-resumable
  (`recover_reclaim_tombstones`, `src/scratch.rs:306-338`), so an interruption
  mid-reclaim leaves at most a tombstone, which the next attempt re-arms.
- **Daemon restart** clears no durable state: the retention policy is
  configuration, the sweep is stateless, and the lease is read from durable
  state on every pass. A receipt left `accepted` by a crashed daemon is marked
  `interrupted` during bootstrap (`src/core/daemon_bootstrap.rs:206-211`), so a
  crashed turn cannot pin a tree forever.
- **Contention with a live turn**: a turn admitted before the sweep's lock is
  visible to the sweep and its tree is not reclaimed. A turn admitted after the
  sweep commits finds the scratch root recreated on demand. Neither loses a
  live `TMPDIR`.
- **Contention with a restore**: a session restored while the sweep is deciding
  is rechecked inside the writer lock; the restore either wins first (the
  sweep sees the row and skips) or waits for the sweep to commit. This is the
  same ordering M004 already relies on.
- **Contention with another reclaim**: the process-wide reclaim lock
  (`src/scratch.rs:12`) serializes removals; concurrent reclaimers of one
  resource remove it exactly once.
- **A tree that vanishes mid-sweep** (a concurrent reclaim removed it) is
  recorded as vanished and does not fail the pass.
- **Budget exhaustion** produces a pass that records it stopped and at which
  budget. A truncated pass is never presented as a complete sweep.
- **The operator reclaim races a live turn**: the operator action refuses a tree
  whose session has in-flight work, exactly as the sweep does. The operator
  cannot force a live tree away.

## 9. Compatibility and migration

- **No storage change.** No migration, no `STORAGE_LAYOUT_VERSION` bump, no
  schema or `codegg-core` table change.
- **The configuration change is additive.** A new optional section; an existing
  config file without it is unchanged and takes the defaults. The
  `scripts/check_config_merge_coverage.py` obligation is satisfied by the merge
  arm in WP2.
- **The protocol change is additive.** `PROTOCOL_VERSION` stays `2`: the
  request is a new variant of an existing enum and a client only sends it
  deliberately. `PROTOCOL_VERSION` must be bumped **only** if the implementation
  cannot keep the request decodable by an older peer — the plan's default is to
  keep it at 2 and make the client degrade gracefully.
- **Version skew across a daemon upgrade** is the one real compatibility case.
  Because the socket server silently drops a frame that does not deserialize
  (`src/core/transport/daemon_socket.rs:445`), a new client against an older
  daemon receives **no response**. The client must therefore use a bounded
  request timeout and report a typed "the running daemon does not support
  scratch reclaim; restart it to upgrade" error. Hanging is not an acceptable
  degradation.
- **The reclaim-validator change alters behaviour on Linux and macOS only.** A
  tree that used to be permanently quarantined is now reclaimed. That is the
  point of the milestone. Windows behaviour is unchanged.
- **The scratch layout, identity rules, and owner-record format are unchanged**,
  so no existing tree is invalidated and no on-disk migration is implied.
- **The disposable-cache wording is preserved verbatim** in
  `architecture/process-tool-execution-ownership.md` and
  `architecture/security.md`.

## 10. Required tests

- `src/scratch.rs` unit tests: a tree containing a hardlink to an outside file
  is reclaimed **and** the outside file's content is unchanged; a tree
  containing two hardlinks to the same inside file is reclaimed; a tree
  containing a planted symlink is still refused and quarantined, and the
  symlink's target is untouched; a tree containing a FIFO is still refused; the
  shared digest function produces the same value as the previous inline
  computation for the same session id.
- `src/core/scratch_sweep.rs` unit and integration tests: a soft-deleted
  session's tree is swept; an idle session older than the retention age is
  swept; a session inside the retention age is not; a session with an
  `accepted` turn receipt is not; a session with a non-terminal run is not; a
  session with a non-terminal job is not; an orphan tree is swept only after
  the grace period; the sweep is bounded and reports that it stopped; a tree
  that vanishes mid-sweep is recorded and does not fail the pass; a restore
  during a sweep is not lost (the writer-lock recheck).
- **The operator path**: a dry run removes nothing and reports the verdict; an
  applied reclaim removes the tree; an applied reclaim of a tree containing a
  symlink to an outside **directory** removes the tree and leaves the outside
  directory's contents intact (the link-safety proof); an applied reclaim
  refuses a tree whose session has in-flight work; an applied reclaim of a
  digest that matches no tree is a clean no-op; the request cannot carry a
  path; the audit record is emitted with bounded metadata.
- **Protocol and authorization**: a socket round-trip; the operation is refused
  for a non-local-owner principal; the operation is not reachable on the remote
  HTTP/WS surface; `scripts/check_authorization_matrix.py` and
  `scripts/check_audit_coverage.py` pass.
- **Config**: the merge arm's behaviour; `retention_days` below the floor is
  rejected or clamped rather than silently accepted.
- **Regression**: the existing reclamation tests in `src/scratch.rs` and
  `src/core/daemon_sessions.rs` (`scratch_cleanup_tests`) still pass unchanged
  apart from the deliberate Unix hardlink expectation in WP1.
  `tests/linux_native_scratch_containment.rs` is unaffected and remains a
  Linux-only native qualification.
- **Negative control**: a test that proves the sweep cannot reclaim a tree with
  in-flight work, and a test that proves the operator path cannot reclaim a
  tree with in-flight work. A sweep that removed a live tree would be worse
  than no sweep at all.
- **Transaction shape**: the extended
  `scripts/check_queue_write_transaction_shape.py --self-test` still fails on
  its injected deferred read-then-write path, and the new sweep module is
  covered by the guard.

## 11. Required verification commands

```bash
cd /Users/davidbowman/projects/worktree/codegg
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo nextest run --locked --profile ci -p codegg --lib -E 'test(/scratch/)'
CARGO_BUILD_JOBS=2 cargo nextest run --locked --profile ci -p codegg --test agent_loop_harness
CARGO_BUILD_JOBS=2 cargo nextest run --locked --profile ci -p codegg -E 'test(/scratch/) or test(/reclaim/) or test(/daemon/) or test(/config/)'
CARGO_BUILD_JOBS=2 cargo nextest run --locked --profile ci -p codegg-core -E 'test(/audit/) or test(/authorization/)'
scripts/verify.sh quick
```

`scripts/verify.sh quick` is required, not optional: it carries the
`check_daemon_cwd_usage`, `check-core-boundary`, `check_execution_ownership`,
`check_config_merge_coverage`, `check_project_catalog_invariants`,
`check_authorization_matrix`, and `check_audit_coverage` guards that a change
in this area can trip. Note that CI is a **subset** of `quick`; a green CI run
does not imply a green `quick`.

## 12. Documentation updates

- `architecture/security.md` — the reclamation paragraph: the retention policy
  and its default, the sweep's cadence and bound, the lease, the orphan rule,
  and the corrected Unix/Windows statement. The disposable-cache wording is
  preserved.
- `architecture/process-tool-execution-ownership.md` — the disposable-cache
  paragraph is preserved verbatim; the bounded retention is mentioned as an
  addition to the contract, not a change to it.
- `docs/tools.md` — the scratch paragraph: the tree is now reclaimed on a
  retention policy, and the operator command that can dispose of a tree the
  automatic path refuses.
- `docs/daemon.md` — the lifecycle-command table gains
  `codegg daemon scratch-reclaim`, with the dry-run default stated.
- `architecture/authorization.md` — the operation-to-capability matrix gains
  the `scratch_reclaim` row, and the operator matrix documents the local-only
  scope.
- `architecture/audit.md` — the `scratch_reclaim` action in the taxonomy, and
  its row in the operator matrix, with the bounded metadata named.
- `codegg.example.jsonc` — the `scratch` section, annotated.
- `.opencode/skills/tool-execution/SKILL.md` — the lease-aware contract, so the
  skill agrees with the architecture doc it points at.
- `plans/subsystems/managed-agent-scratch-roadmap.md` — the M007 milestone
  section and the §12 status row.
- `plans/registry.md` — the M007 row.

## 13. Acceptance criteria

These are externally observable and are what the closure record must be able to
point at.

1. A session that has been idle longer than the retention age has its scratch
   tree reclaimed by the daemon, with no human action.
2. A session that is **inside** the retention age keeps its tree.
3. A session with an admitted turn, a non-terminal run, or a non-terminal job
   keeps its tree — proven by test, and by a negative control.
4. A soft-deleted session's tree is reclaimed, and the behaviour survives a
   daemon restart.
5. An orphan tree — one whose digest matches no durable session — is reclaimed
   after the grace period, and not before.
6. A tree containing a multiply-linked file is reclaimed on Linux and macOS,
   and the outside file it pointed at still has its original content
   afterwards. This is the specific defect §3.3 identifies.
7. A tree containing a planted symlink is still refused by the automatic path,
   and the symlink's target is untouched.
8. `codegg daemon scratch-reclaim --tree <digest>` reports what it would
   remove and removes nothing; with `--apply` it removes the named tree, and
   the removal is recorded in the audit trail with bounded metadata.
9. The operator command refuses a tree whose session has in-flight work.
10. The sweep never blocks a tool call, a permission check, a turn admission, or
    a scheduler decision; a test observes that the tool path performs no walk.
11. No sweep and no operator reclaim removes a tree outside the daemon's own
    scratch root, and no removal relaxes the owner-record or containment check.
12. `scripts/verify.sh quick`, the focused suites, and the hosted CI run on the
    pull-request head are green.
13. The closure record states plainly that the retention **default** was
    specified from the contract and validated (or not validated) against M006's
    census, and names which of the two it was.

## 14. Stop conditions

The implementing agent must **stop and report** rather than improvise if:

- The parity fix (WP1) cannot be shown to leave the outside file's content
  intact by an executable test. The safety argument is not a substitute for the
  test.
- The retention age cannot be derived from durable state, and the only
  available signal would require a filesystem walk on the decision path. That
  would put a traversal on the sweep's hot path and is the shape the roadmap
  forbids.
- A durable column that marks a session's last activity turns out not to be
  bumped by turn activity, and no alternative durable signal exists. Report
  which column was checked and what it does.
- The sweep would need to reclaim a tree whose session cannot be proven idle.
  Refusing to sweep is correct; guessing is not.
- A reclaim would need to weaken the owner-record check, the containment check,
  or the tombstone path to succeed.
- The operator action would need to be reachable from the remote HTTP/WS
  surface, or from a principal that is not the local owner.
- An audit record for the operator action would need unbounded metadata, an
  absolute path, or file content.
- The transaction shape requires a deferred transaction on a read-then-write
  path. That is the C002 hazard; extend the guard instead.
- Any claim would require Windows native sandbox qualification. This milestone
  claims no Windows containment.
- The parity fix turns out to require a change to the sandbox grant set. That
  is an authority change: stop and propose an ADR.
- WP0's census cannot be obtained. Proceed with §6.1's defaults and record the
  measurement as unobtained — never as a passing one.

## 15. Closure evidence required

- The M006 census output (WP0), with the environment named, and the resulting
  decision about the defaults.
- A before/after manifest of the scratch root across a sweep, showing the
  intended removals and nothing else.
- The focused test results, including every negative test in §10 — especially
  the three lease negatives and the hardlink-safety proof.
- `scripts/verify.sh quick` green, and the hosted CI run ID on the
  pull-request head and on the merged head.
- The audit record for an applied operator reclaim.
- The registry and roadmap reconciled with the closure record's status.
- An explicit statement of what the milestone does **not** deliver: no byte
  eviction, no new lease primitive, no change to the symlink quarantine, no
  Windows containment claim, and no change to what a model, a tool, or a human
  may do.

## 16. Handoff notes

- The most important property is that this milestone **reclaims only the
  daemon's own disposable scratch**. If an implementation path appears to need
  to delete anything outside the scratch root, or to relax an ownership check,
  that is a scope error: stop.
- The Unix multiply-linked refusal is a **defect with the fix already written
  down in the code's own comment** (`src/scratch.rs:405-410`). The implementing
  agent should read that comment and the Windows test before designing
  anything; there is no design decision left here, only a change and a proof.
- The lease needs **no new primitive**. The proven pattern is
  `reclaim_deleted_session_scratch_when_idle`
  (`src/core/daemon_sessions.rs:1202-1254`); M007 generalizes it from "deleted"
  to "idle by age" and adds the `accepted` turn receipt.
- The sweep must map durable→disk in **reverse** — enumerate durable session
  ids and hash each with the same function `session_scratch_dir` uses — because
  the dir name is a one-way digest and the owner record stores the digest, not
  the session id. Factor that computation into one named function; a second
  copy is how the mapping silently breaks.
- The sweep **reads before it writes**. That is the C002 hazard. Use
  `begin_immediate`, promote the helper, and extend
  `scripts/check_queue_write_transaction_shape.py` to the new module.
- The operator action's safety rests on `std::fs::remove_dir_all` not
  following symbolic links. That is a real guarantee, and it is only accepted
  with the test in §10.
- The default retention is 14 days. It is a starting point derived from the
  disposal contract, not from a measurement; WP0's census is what turns it
  into a measured value, and the closure record must say which it is.
