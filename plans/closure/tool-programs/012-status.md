# Tool Programs Milestone 012 — Closure Status

Status: historical conditionally closed — implementation record; strict closure transferred to M013

Source implementation plan:

- `plans/implementation/tool-programs/012-authority-recovery-and-delivery-corrective-closure.md`

Strict-closure successor:

- `plans/implementation/tool-programs/013-production-authority-descendant-and-recovery-closure.md`

Subsystem authority:

- `plans/subsystems/tool-programs-correctness-closure-addendum.md`

Original M012 planning baseline:

- `f26fb687857390431b5eaabc212583b4b20da30d`

Actual implementation commit:

- `d056e4236e1ef10b4639b8bbf05557090dc6112c` — `fix(tool-programs): M012 authority, broker, notification, lineage, recovery, result, and hosted corrections`

## 1. Disposition

M012 remains a useful implementation record. It improved Broker terminal-outcome handling, introduced typed grant and artifact structures, added initial SQLite transition logic and child-lineage fields, restored completed-call data, added result-load checks, and made the model-facing backend policy explicitly `native_only`.

A post-implementation production-path review found that the milestone does not establish strict closure. Several claimed invariants are incomplete in production or are supported only by structural tests. M013 is therefore the sole active strict-closure authority.

## 2. Corrected findings

### High — authority is not yet a persisted permission decision

The grant is still generated from execution-context and program data inside the executor rather than created from the actual accepted permission and workspace path-policy decision before job admission. The Broker does not verify the complete grant scope against each nested invocation.

M013 owner: Work Packages A and B.

### High — notification delivery is not yet proven transactionally

The SQLite transition path requires correction and tests currently do not use independent service instances sharing one database. Restart behavior across claim, durable session insertion, and acknowledgement is not established.

M013 owner: Work Package C.

### High — child lineage and descendant ownership are not durable

Lineage values are populated at submission but are not fully stored and reconstructed by the job stores. The scheduler does not yet own recursive descendant cancellation and restart reattachment independently of the parent executor future.

M013 owner: Work Packages D and E.

### High — checkpoint and replay closure are incomplete

Completed calls are reloaded, but the checkpoint is not restored and replay is not bound to the complete authority, context, workspace, manifest, contract, source, IR, backend, deadline, call-order, and child identity.

M013 owner: Work Packages F and G.

### Medium-high — result and artifact integrity are partial

Child artifacts remain incomplete and the stored digest does not authenticate the full semantic result record.

M013 owner: Work Package H.

### High evidence gap — process-boundary recovery is not demonstrated

The M012-specific tests do not exercise a fresh daemon instance against the same durable state, independent SQLite services, active-child reattachment, or complete scheduler resource convergence.

M013 owner: Work Package J.

### Governance correction

The earlier record identified `f26fb68` as the implementation commit. That commit registered M012; the implementation commit is `d056e4236e1ef10b4639b8bbf05557090dc6112c`.

## 3. Corrected criteria disposition

| M012 area | Corrected result | M013 successor criteria |
|---|---|---|
| Authority construction and verification | not closed | C-01–C-08 |
| Broker non-success terminal mapping | substantially implemented; retain regression coverage | C-07–C-08 |
| Transactional notification delivery | not closed | C-09–C-14 |
| Durable lineage and descendant ownership | not closed | C-15–C-22 |
| Checkpoint, replay fingerprint, and deadline recovery | partially implemented; not closed | C-23–C-30 |
| Typed result and artifact integrity | partially implemented; not closed | C-31–C-36 |
| Production backend truthfulness | model-facing native-only policy retained; internal behavior requires final proof | C-37–C-38 |
| Process-level evidence | not closed | C-39–C-42 |
| No unresolved high or medium findings | failed | C-43 |
| Documentation and evidence agreement | transferred to independent M013 review | C-44–C-45 |

## 4. Historical test evidence

The M012 implementation record reported formatting and compilation success, all M012-specific tests passing, and a broad workspace test run with one described pre-existing failure and one skipped suite.

The post-M012 review did not independently rerun those commands and found no attached GitHub status checks for the implementation commit. The reported commands remain historical author-provided evidence, not independent strict-closure evidence.

## 5. Operational claims pending M013

Until M013 closes, documentation must not claim:

- authority derived from and verified against a real permission decision;
- transactionally exactly-once notification delivery across independent services and restart;
- durable scheduler-queryable child lineage;
- scheduler-owned recursive descendant convergence;
- complete checkpoint restoration and replay fingerprint validation;
- complete child artifact and full-record result integrity;
- process-boundary restart closure.

The read-only programmable palette and `native_only` production policy remain in force.

## 6. Final status

M012 is **historical conditionally closed** as an implementation record. M013 owns the remaining production authorization, delivery, descendant, recovery, integrity, resource-convergence, evidence, and governance closure work.