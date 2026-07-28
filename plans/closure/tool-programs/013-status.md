# Tool Programs Milestone 013 — Closure Status

Status: historical conditionally closed — implementation record; strict closure transferred to M014

Source implementation plan:

- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`

Strict-closure successor:

- `plans/implementation/tool-programs/014-production-boundary-and-process-evidence-closure.md`

Subsystem authority:

- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

M013 implementation range:

- baseline before M013 implementation: `e2141880a33c151e78147444f2758d50a779282b`
- final M013 implementation/closure commit reviewed: `58e87ff3d82508037ae4912df2ae9b9b8a4ef090`

## 1. Disposition

M013 remains a substantial implementation record. It improved Broker terminal-outcome handling, persisted grants in Tool Program payloads, added grant integrity checks, corrected notification state token storage and compare-and-set syntax, added basic SQLite lineage columns/indexes, expanded replay fingerprints, replaced several mislabeled MD5 ledger digests, strengthened typed result hashing, and retained explicit `native_only` production policy.

A post-implementation production-path review found that M013 does not establish strict closure. Several closure claims exceed the implemented mechanisms, mandatory process evidence was explicitly deferred, and the implementation pass created and accepted its own closure record despite the plan prohibiting self-closure. M014 is therefore the sole active strict-closure authority.

The original M013 closure claims remain available in Git history at `58e87ff3`; this reconciliation does not erase that evidence trail.

## 2. Corrected findings

### High — authority remains synthesized rather than decision-derived

`to_core_context()` still constructs principal, authority, workspace path-policy, and policy-revision values from program/workspace/session/agent strings. `build_authority_grant()` hashes that context instead of consuming the actual accepted permission and path-policy decision from the direct Tool Program invocation.

M014 owner: Work Package A; criteria C-01 through C-06.

### High — production contract snapshot is internally inconsistent

The normal submission path computes the grant contract digest from an empty contract summary. Broker verification computes a digest from the concrete invoked contract and requires equality. Isolated hand-built authority fixtures therefore do not establish that an ordinary authorized nested call succeeds through production submission.

M014 owner: Work Package B; criteria C-07 through C-10.

### High — checkpoint restoration is not wired and checkpoint state is incomplete

The executor loads completed calls but does not load the latest durable checkpoint or call `restore_checkpoint()`. The checkpoint stores a locals hash rather than the bounded locals/control state required to resume safely. The production replay fingerprint also sets `original_deadline_millis` to `None`.

M014 owner: Work Package C; criteria C-11 through C-20.

### High — lineage and descendant ownership remain incomplete

The durable job model lacks parent program ID, instruction sequence, and relation kind. Child call identity remains operation-derived. Common in-memory transitions erase lineage fields. Lineage schema changes were added to an already-applied migration, so existing databases do not receive them. Descendant enumeration and cancellation are direct-only rather than recursive.

M014 owner: Work Packages D and E; criteria C-21 through C-30.

### High — notification persistence does not fail closed

Notification persistence still returns `()` and logs serialization or SQLite errors. Recovery still computes MD5 payload digests for a field documented as SHA-256. Durable injection identity and parent-session idempotency are not fully enforced through schema and the real session insertion boundary.

M014 owner: Work Package F; criteria C-31 through C-38.

### Medium-high — result artifacts are not canonical or complete

Child artifact records still lack real run identity, artifact handles, and digests. Large output is written directly to a constructed filesystem path and represented by a fabricated `ctx://` string rather than a canonical artifact-store operation. Artifact write failure logs and continues.

M014 owner: Work Package G; criteria C-39 through C-44.

### High — replay storage remains process-local in its concurrency guarantees

The new per-program `DashMap` mutex only serializes writers inside one process. It does not protect overlapping daemon processes or crash/restart boundaries around whole-file journal updates.

M014 owner: Work Package C; criterion C-19.

### High evidence gap — the required daemon process harness was deferred

The M013 process suite reconstructs stores and services inside one test process. It does not submit through a public daemon protocol, activate failpoints, kill the daemon, restart a fresh process against the same state, reattach active children, or prove managed process-group cleanup.

M014 owner: Work Package H; criteria C-45 through C-49.

### Governance correction

The M013 implementation plan required the implementation pass to move only to `closing` and prohibited it from creating or approving `plans/closure/tool-programs/013-status.md`. Commit `58e87ff3` combined additional implementation with creation and acceptance of the closure record, while that record marked a mandatory binary process criterion as deferred.

M014 owner: Work Package I; criteria C-50 through C-54.

## 3. Corrected criteria disposition

| M013 area | Corrected result | M014 successor criteria |
|---|---|---|
| Real permission/path-policy authority | not closed | C-01–C-06 |
| Canonical contract snapshot and normal nested-call success | not closed | C-07–C-10 |
| Checkpoint restoration and original deadline | not closed | C-11–C-20 |
| Durable complete lineage and recursive descendants | not closed | C-21–C-30 |
| Transactional fail-closed notification delivery | partially implemented; not closed | C-31–C-38 |
| Canonical call/child/output artifacts | partially implemented; not closed | C-39–C-44 |
| Native-only backend truthfulness | retained | regression coverage under M014 |
| Real daemon kill/restart evidence | not implemented | C-45–C-49 |
| No unresolved high or medium findings | failed | C-51 |
| Independent closure governance | failed | C-52–C-54 |

## 4. Historical test evidence

M013 reported targeted formatting, compilation, 106 M013 tests, and 36 Broker/contract tests passing. The post-M013 review did not independently rerun those commands and found no attached GitHub workflow run for the reviewed commit. Those claims remain historical author-reported evidence, not independent strict-closure evidence.

The presence of passing component tests does not override the production-path mismatches and deferred mandatory process criterion described above.

## 5. Operational claims pending M014

Until M014 closes, documentation must not claim:

- authority derived from the actual accepted permission/path-policy decision;
- a canonical contract snapshot that permits normal authorized nested calls and detects drift consistently;
- complete checkpoint restoration or original-deadline recovery;
- complete immutable lineage across all job transitions and upgrades;
- recursive scheduler-owned descendant convergence;
- fail-closed exactly-once notification delivery through the real session boundary;
- canonical, resolvable, digest-verifiable child and output artifacts;
- cross-process-safe replay/checkpoint storage;
- daemon kill/restart closure.

The read-only programmable palette and `native_only` production policy remain in force.

## 6. Final status

M013 is **historical conditionally closed** as an implementation record. M014 owns the remaining production authority, contract, checkpoint, lineage, recursive descendant, notification, artifact, process-evidence, and governance closure work.