# Tool-Selection Advisor Retrieval-Architecture Closure Corrective C001 — Execution Ownership and Planning Closure

Status: ready for handoff

Repository baseline: `5864e5497d23f65fdec0390ac65c4173d92af2dd`

Source corrective:

- `plans/subsystems/tool-selection-advisor-retrieval-architecture-closure-corrective-addendum.md`

Original milestone/closure:

- `plans/implementation/tool-selection-advisor-retrieval-architecture-experiment/003-extended-frontier-and-operating-point-selection.md`
- `plans/closure/tool-selection-advisor-retrieval-architecture-experiment/003-status.md`

Controlling architecture/process:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`
- `plans/003-planning-process.md#7-corrective-passes`

Primary class: correctness/closure corrective.

## 1. Objective

Restore repository-clean closure without changing the retrieval experiment's technical result.

The pass must:

1. remove the ungoverned `git rev-parse HEAD` subprocess from `src/tool_advisor/retrieval_architecture.rs`;
2. preserve implementation-commit provenance in R001 receipts by explicit caller/operator injection;
3. reconcile the retrieval-architecture roadmap to its actual closed state;
4. run the canonical execution-ownership/quick verification locally;
5. require a green hosted CI run before final closure.

## 2. Defects to close

### D1 — execution-ownership guard failure

Current code:

```rust
let output = Command::new("git")
    .arg("-C")
    .arg(root)
    .arg("rev-parse")
    .arg("HEAD")
    .output()
```

Current head CI run `35829020175` fails at `Execution ownership guard`.

The experiment module is not an execution owner. Provenance capture does not justify adding another process lifecycle.

### D2 — stale roadmap status

`plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md` still says:

```text
Status: active
```

while its M003 closure and the registry both state that the workstream is closed.

### D3 — duplicate stale milestone row

The roadmap milestone table contains both a closed M002 row and a stale:

```text
| M002 | not started | — | — | M001 |
```

The stale row must be removed.

### D4 — incomplete closure verification

The original closure did not run `scripts/verify.sh quick`, allowing D1 to reach hosted CI.

The corrective closure must record both local canonical guard evidence and hosted CI.

## 3. Production-code correction

### 3.1 Remove repository-owned Git process execution

Remove the `std::process::Command` dependency and `resolve_implementation_commit(root)` subprocess helper from `retrieval_architecture.rs`.

Do **not** fix this merely by adding `retrieval_architecture.rs` to `docs/execution-ownership.toml`. The module does not own process execution and should not acquire that authority solely for experiment metadata.

### 3.2 Explicit provenance injection

Change the long-running R001 sweep boundary so implementation commit identity is an explicit input.

Preferred contract:

```rust
run_r001_sweep(output_dir: &Path, implementation_commit: &str)
```

or an equivalent typed sweep-config field.

Validate the supplied value before fingerprinting:

- non-empty;
- hexadecimal;
- 40-character Git SHA-1 or 64-character SHA-256 form;
- normalized lowercase in receipts/fingerprints.

The existing sweep fingerprint continues to include this supplied commit identity exactly as before.

No fallback to spawning Git is permitted.

### 3.3 Operator/test command

The ignored long-running test or operator harness may read an explicit environment variable such as:

```text
CODEGG_R001_IMPLEMENTATION_COMMIT
```

and pass it into the sweep function.

The documented local invocation may use the shell to obtain the SHA outside CodeGG:

```bash
CODEGG_R001_IMPLEMENTATION_COMMIT="$(git rev-parse HEAD)" \
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- \
  tool_advisor::retrieval_architecture::tests::r001_extended_frontier_sweep \
  --ignored --nocapture
```

The Rust process itself MUST NOT launch Git.

A missing/invalid provenance variable fails fast before encoder load or sweep work.

## 4. Regression tests

Add focused tests proving:

- valid 40-char hexadecimal SHA accepted;
- valid 64-char hexadecimal SHA accepted;
- empty value rejected;
- non-hex value rejected;
- wrong-length value rejected;
- normalized commit identity participates in the sweep fingerprint;
- changing only implementation commit changes the sweep fingerprint;
- no implicit fallback obtains a commit from the repository.

The repository execution-ownership guard is the recurrence guard for accidental direct subprocess introduction.

Do not add a brittle source-text test duplicating the canonical guard unless needed for a specific local invariant.

## 5. Evidence preservation

The previous 2700-point coarse frontier and M003 negative result MUST NOT be rerun merely because provenance plumbing changed.

The corrective closure must state why historical evidence remains valid:

- retrieval algorithms unchanged;
- fixtures unchanged;
- preregistered grid/gates unchanged;
- candidate/ranker artifacts unchanged;
- only commit-SHA acquisition moved from internal subprocess to explicit input.

Do not rewrite `plans/closure/tool-selection-advisor-retrieval-architecture-experiment/003-status.md`.

Add:

- `plans/closure/tool-selection-advisor-retrieval-architecture-closure-corrective/001-status.md`

as supplemental closure evidence.

## 6. Planning-state reconciliation

Update:

### Retrieval-architecture roadmap

`plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md`

Required final state:

- top-level `Status: closed`;
- M001 closed;
- M002 closed;
- M003 closed negative;
- remove duplicate stale `M002 | not started` row;
- completion definition/result remains negative: no operating point, no v4 plan, nothing downstream unblocked.

Do not change the historical technical conclusions.

### Corrective addendum

Mark the closure corrective closed only after hosted CI is green.

### Registry

At implementation start:

- corrective C001 active.

At closure:

- corrective C001 closed with implementation commit + closure record + hosted CI run;
- original retrieval-architecture workstream remains closed;
- order-invariance M005 remains blocked.

## 7. Verification

Run locally before pushing closure:

```bash
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- \
  tool_advisor::retrieval_architecture::
cargo clippy --locked --features tool-advisor-encoder-training -p codegg --lib -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Because `scripts/verify.sh quick` already runs formatting and a workspace check, record its exact outcome rather than claiming broader tests that were not run.

After push, require the ordinary GitHub `CI / verify` workflow to complete successfully. Record run ID/URL in the corrective closure.

If hosted CI fails for any reason, C001 remains open until the failure is classified and resolved or separately registered.

## 8. Security and authority

- No new execution owner.
- No scheduler/daemon/process-service changes.
- No new network path.
- No secrets or user data.
- Experiment provenance remains explicit and auditable.
- Advisor authority and `ResolvedToolSurface` behavior untouched.

## 9. Migration and compatibility

No user-facing schema, protocol, storage, or runtime migration.

Changing the experiment-only sweep function signature is acceptable. Update every internal test/call site atomically.

Checkpoint compatibility:

- existing historical checkpoints remain historical evidence;
- a future rerun under a different implementation commit naturally gets a different sweep fingerprint;
- do not resume an old checkpoint under the new commit identity.

## 10. Stop conditions

Stop and register a follow-up rather than widening scope if:

- removing the Git subprocess requires changing scheduler/process ownership;
- a retrieval/ranking algorithm change becomes necessary;
- historical sweep evidence is found to depend on the Git subprocess semantics;
- hosted CI exposes an unrelated substantive repository failure requiring its own corrective.

Do not use C001 to start descriptor/query-signal research.

## 11. Acceptance

C001 closes only when all are true:

- no `Command::new("git")` or equivalent process spawn remains in `retrieval_architecture.rs`;
- explicit commit provenance is validated and fingerprint-bound;
- focused provenance regression tests pass;
- execution-ownership guard passes;
- `scripts/verify.sh quick` passes;
- focused retrieval tests, Clippy, fmt, and diff-check pass;
- roadmap status/table are reconciled;
- hosted CI passes;
- additive corrective closure is committed;
- no technical retrieval conclusion or downstream blocker changed.

After positive closure, the next substantive advisor work—if pursued—requires a **separate** retrieval-signal experiment. This corrective itself unblocks nothing.
