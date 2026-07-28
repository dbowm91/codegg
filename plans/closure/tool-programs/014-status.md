# M014 — Production-Boundary and Process-Evidence Closure

Status: historical conditionally closed — implementation record; strict closure transferred to M015

Implementation head reviewed:

- `c9559d23634771dc1bae742da43ae8e362507f6f`

Post-implementation review disposition:

- M014 landed substantial production mechanics and is retained as a valuable implementation record.
- M014 did not satisfy strict closure because several closure-bearing production mechanisms and the process evidence remained incomplete or internally inconsistent.
- Final strict-closure ownership is transferred to `plans/implementation/tool-programs/015-final-production-path-and-independent-closure.md`.

## 1. Retained implementation value

M014 materially improved the subsystem by adding or strengthening:

- persisted Tool Program authority grants in job payloads;
- executor-side grant integrity and validity verification;
- expanded checkpoint state and executor checkpoint loading;
- expanded replay fingerprints including the original job deadline;
- v35 lineage schema fields and preservation through common state transitions;
- attempted recursive descendant enumeration;
- SHA-256 result-record integrity verification;
- native-only backend enforcement;
- file-locking work for replay state;
- additional targeted test files and planning documentation.

These changes remain part of the baseline for M015 and should not be reverted without a failing production-path test.

## 2. Post-implementation findings

### F01 — Executable authority can still be synthesized

The production context and grant constructors can substitute program, workspace, session, or agent-derived values when accepted permission/path-policy decision fields are absent. Those values are correlation material, not proof of authorization.

The M014 authority tests construct workspace contexts and grants directly. They do not prove that the normal accepted direct-call decision creates the persisted grant or that a denied/missing decision creates no source record or scheduler job.

Transferred to M015 work package A and criteria C-01 through C-05.

### F02 — Contract digest verification is incompatible

Submission computes a canonical full-catalog snapshot digest. Broker scope verification computes a different per-tool legacy digest over fewer fields and compares it directly with the catalog digest. Normal authorized nested calls are therefore not proven and can fail with contract mismatch.

Submission also creates a separate default registry and converts contract-resolution errors into an empty snapshot.

Transferred to M015 work package A and criteria C-06 through C-10.

### F03 — Checkpoint restoration can erase newer call completions

The executor loads completed calls and then restores a checkpoint. Checkpoint restoration replaces the completed-call map. A crash after durable call completion but before the next checkpoint can therefore cause restart to forget and repeat the call.

Transferred to M015 work package B and criteria C-11 through C-14.

### F04 — Active child wait identity is not persisted

The checkpoint type includes pending-child state, but production checkpoint creation writes `None`, and child execution waits for terminal completion before committing the next checkpoint. Restart reattachment and duplicate-submission prevention are not established.

Transferred to M015 work package B and criteria C-15 through C-20.

### F05 — Child and large-output artifacts are not canonical

Child tracking uses a digest of job ID and status, leaves run identity absent, and returns no real child artifacts. Large output is written directly to a constructed filesystem path, receives a manually fabricated `ctx://` handle, and logs storage failure rather than failing result commit.

The M014 artifact tests manually assemble handle and digest-shaped values rather than producing and resolving them through the production stores.

Transferred to M015 work package C and criteria C-21 through C-28.

### F06 — Notification persistence does not fail closed

Notification creation inserts into memory before durable persistence, logs SQLite errors, and returns apparent success. Recovery can convert a database error into zero recovered records. The executor cannot distinguish successful durable notification creation from warning-only failure.

Transferred to M015 work package D and criteria C-29 through C-36.

### F07 — Descendant traversal stops at terminal intermediate nodes

The recursive query filters terminal children before adding them to the traversal queue. An active grandchild beneath a terminal intermediate job is not discovered. Complete process-group, permit, lease, counter, and capacity convergence was not demonstrated.

Transferred to M015 work package E and criteria C-37 through C-42.

### F08 — Daemon process evidence is nominal

The M014 daemon suite:

- verifies binary presence without public Tool Program submission;
- starts and kills a daemon without readiness, failpoint, or durable recovery assertions;
- returns success when spawning fails;
- uses multiple ledger objects inside one process as restart evidence;
- does not prove child reattachment, append-before-ack recovery, process cleanup, or resource convergence.

Transferred to M015 work package F and criteria C-43 through C-48.

### F09 — Closure governance was self-created and contradictory

The implementation commit also created this closure record. The original text identified an independent reviewer and “this commit,” marked the status `closing`, and concluded that M014 was closed. No separate independent closure commit or attached GitHub status evidence established that claim.

Transferred to M015 work package G and criteria C-49 through C-52.

## 3. Corrected criteria disposition

The original M014 C-01 through C-54 claim is not accepted as strict closure.

- authority decision provenance: not closed;
- canonical contract convergence and successful authorized nested execution: not closed;
- checkpoint type expansion and loading: substantially implemented;
- monotonic call/checkpoint recovery: not closed;
- pending-child persistence and restart reattachment: not closed;
- v35 lineage migration and common transition preservation: substantially implemented;
- complete recursive descendant/resource convergence: not closed;
- notification CAS syntax and SHA-256 work: partially implemented;
- fail-closed notification persistence and exactly-once session injection: not closed;
- result-record integrity: substantially implemented;
- canonical call/child/output artifacts: not closed;
- native-only production policy: retained;
- real daemon failpoint/restart evidence: not closed;
- independent closure governance: not closed.

## 4. Test evidence disposition

The M014 implementation commit reports:

- seven M014 test files and 54 passing tests;
- `cargo fmt --all -- --check` passing;
- `cargo check -p codegg --all-targets` passing;
- static guard scripts passing.

These are author-reported implementation results. The post-implementation review did not find attached GitHub workflow runs or combined status checks for `c9559d23634771dc1bae742da43ae8e362507f6f`, and several tests do not exercise the mechanisms named by their criteria.

The retained tests may be useful regression coverage, but they cannot serve as final closure evidence without M015 production-path replacement or augmentation and independent rerun.

## 5. Final status

M014 is conditionally closed as a historical implementation record.

Strict Tool Programs closure is owned exclusively by:

- `plans/implementation/tool-programs/015-final-production-path-and-independent-closure.md`

M015 must remain `ready` or `closing` until a separate reviewer creates and accepts:

- `plans/closure/tool-programs/015-status.md`

No document should claim strict native-only Tool Programs closure until all M015 C-01 through C-52 criteria are independently verified at the exact reviewed implementation head.