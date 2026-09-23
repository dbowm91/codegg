# Tool-Selection Advisor Retrieval-Architecture Closure Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-architecture-closure-corrective/001-execution-ownership-and-planning-closure.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-architecture-closure-corrective-addendum.md#C001`
- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md`

Repository baseline reviewed: `220d3638fe043b24e65a7817241543612f9e82af`

Implementation commits or pull requests:

- `220d3638` — retrieval-architecture C001: explicit sweep provenance, remove git subprocess

Hosted CI:

- `CI / verify` run `35859251981` — success on `220d3638` (`https://github.com/dbowm91/codegg/actions/runs/35859251981`)
- `CI / verify` run `35861891751` — success on the closure commit `6e183045`
  (`https://github.com/dbowm91/codegg/actions/runs/35861891751`; first attempt hit an
  unrelated flake in `tests/interactive_process_sessions.rs`
  `environment_overrides_apply_while_denied_vars_stay_stripped`, green on `--failed` rerun —
  see §10)

Predecessor evidence (immutable, not rewritten):

- `plans/closure/tool-selection-advisor-retrieval-architecture-experiment/003-status.md` — M003 negative recall verdict (ceiling-proven)
- Hosted CI run `35829020175` — predecessor failure at `Execution ownership guard` (the defect C001 closes)

## 1. Executive finding

C001 is closed. The experiment-owned `git rev-parse HEAD` subprocess is removed from
`src/tool_advisor/retrieval_architecture.rs`; sweep provenance is now an explicit
caller-supplied commit SHA that is validated and fingerprint-bound; the retrieval-architecture
roadmap reconciles to its actual closed state (stale `Status: active` fixed, duplicate
`M002 | not started` row removed); canonical local guards pass; and hosted `CI / verify`
is green on the implementation commit.

The M003 negative technical result is unchanged and was not rerun. No downstream advisor
milestone is unblocked by this corrective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| D1: remove ungoverned `git rev-parse HEAD` subprocess | `resolve_implementation_commit` + `use std::process::Command` deleted from `retrieval_architecture.rs`; `python3 scripts/check_execution_ownership.py` ok; hosted `Execution ownership guard` passes in run `35859251981` | pass | No entry added to `docs/execution-ownership.toml`; the module owns no process execution |
| Explicit provenance injection with validation | `run_r001_sweep(output_dir, implementation_commit)` + `validate_implementation_commit`: non-empty, 40/64-char hex, lowercase-normalized; validation is the first statement, before fixture/encoder/sweep work | pass | Signature change updated atomically at every call site (one ignored test) |
| Operator/test command via explicit env var | Ignored `r001_extended_frontier_sweep` reads `CODEGG_R001_IMPLEMENTATION_COMMIT`, validates it, then passes it in; missing/invalid fails fast before encoder load | pass | Documented invocation: `CODEGG_R001_IMPLEMENTATION_COMMIT="$(git rev-parse HEAD)" cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_architecture::tests::r001_extended_frontier_sweep --ignored --nocapture` |
| Provenance regression tests | 5 new tests (see §4); focused suite 48 passed, 0 failed, 2 ignored (pre-existing ignored sweeps) | pass | Covers 40/64 acceptance, empty/non-hex/bad-length rejection, normalization, fingerprint binding/commit-sensitivity, fail-fast with no repo fallback |
| D2: roadmap stale status reconciled | `tool-selection-advisor-retrieval-architecture-experiment-roadmap.md` top-level `Status: closed` | pass | Historical technical conclusions untouched |
| D3: duplicate stale milestone row removed | Stale `\| M002 \| not started \| — \| — \| M001 \|` row deleted; M001/M002/M003 closed rows retained | pass | — |
| D4: canonical local + hosted verification | `scripts/verify.sh quick` passed locally; hosted run `35859251981` success | pass | Exact commands/results in §4; no broader tests claimed |
| Evidence preservation (no rerun) | 2700-point frontier not rerun; §3 records why history stays valid | pass | Predecessor `003-status.md` not rewritten |
| Additive closure record | This file at `plans/closure/tool-selection-advisor-retrieval-architecture-closure-corrective/001-status.md` | pass | Supplemental only |

## 3. Production implementation evidence

Landed in `220d3638` (`src/tool_advisor/retrieval_architecture.rs`):

- Deleted `use std::process::Command` and the `resolve_implementation_commit(root)` subprocess helper.
- Added `pub fn validate_implementation_commit(raw: &str) -> Result<String>`: trims, rejects empty,
  requires length 40 or 64, requires ASCII hex, returns lowercase. Error messages name the
  implementation commit.
- Changed `pub fn run_r001_sweep(output_dir: &Path)` to
  `pub fn run_r001_sweep(output_dir: &Path, implementation_commit: &str)`; the first line
  validates and normalizes, so the normalized identity is what `r001_sweep_fingerprint` and
  `R001SweepReport.implementation_commit` / `R001FrozenPoint.implementation_commit` bind.
- Updated the ignored `r001_extended_frontier_sweep` harness to read
  `CODEGG_R001_IMPLEMENTATION_COMMIT` from the environment, validate it, and pass it in.
  The shell resolves `$(git rev-parse HEAD)` outside CodeGG; the Rust process never spawns Git.
- Updated `PREREG_SWEEP_COMMAND`, the module header docs, `run_r001_sweep` docs, and the
  ignored-test docs to the env-var invocation. No retrieval scoring, fusion, pooling, universe,
  K, gate, ranker, promotion, or fixture constant changed.

Why historical evidence remains valid (no rerun):

- Retrieval algorithms unchanged (BM25/semantic/RRF/normalized-union, pooling, fusion dispatch).
- Fixtures unchanged (`EXPECTED_FIXTURE_FINGERPRINT`, dev partition tripwire, `M004_*` constants).
- Preregistered grid/gates unchanged (protocol `r001-preregistered-retrieval-architecture-v1`).
- Candidate/ranker artifacts unchanged (frozen span-packed ranker binding intact).
- Only commit-SHA acquisition moved from an internal subprocess to an explicit validated input;
  the fingerprint binds the same normalized identity as before.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_architecture
cargo clippy --locked --features tool-advisor-encoder-training -p codegg --lib -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Long-running sweep invocation contract (documented, not executed — historical evidence stands):

```bash
CODEGG_R001_IMPLEMENTATION_COMMIT="$(git rev-parse HEAD)" \
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- \
  tool_advisor::retrieval_architecture::tests::r001_extended_frontier_sweep \
  --ignored --nocapture
```

### Results

- `python3 scripts/check_execution_ownership.py` — ok (was failing before C001 with the
  `retrieval_architecture.rs:1771` unclassified `Command::new("git")` site).
- `scripts/verify.sh quick` — passed locally (`==> Quick verification passed.`).
- Focused retrieval-architecture tests with `tool-advisor-encoder-training` — 48 passed,
  0 failed, 2 ignored (the 2 ignored are the pre-existing long-running sweep harnesses).
  New provenance tests, all passing:
  - `implementation_commit_accepts_valid_sha1_and_sha256`
  - `implementation_commit_rejects_empty_nonhex_and_bad_length`
  - `implementation_commit_normalizes_case_for_fingerprint`
  - `sweep_fingerprint_binds_normalized_commit_identity`
  - `sweep_rejects_invalid_provenance_before_encoder_work`
- `cargo clippy ... -D warnings` — clean.
- `cargo fmt --all -- --check` — clean; `git diff --check` — clean.
- Hosted `CI / verify` run `35859251981` on `220d3638` — success, including
  `Execution ownership guard`, `Formatting`, `Workspace Clippy`, and `Workspace tests`.
  This is the green run the plan requires before closure.
- Hosted `CI / verify` run `35861891751` on the closure commit — success on `--failed`
  rerun (first attempt failed only on the unrelated PTY env test classified in §10;
  all guards including execution ownership passed on both attempts).

No workspace-wide sweep beyond the above is claimed. The 2700-point R001 frontier was
deliberately not rerun per §5 of the source plan.

## 5. Invariant review

- No new execution owner: the fix deletes a spawn site instead of classifying one; no
  scheduler/daemon/process-service change.
- ADR-0009 holds: deferred-only input, BM25 fallback, no authority-boundary change;
  `ResolvedToolSurface` behavior untouched (no production discovery-path delta).
- No new network path; no secrets or user data; provenance remains explicit and auditable.
- Checkpoint compatibility preserved: old checkpoints remain historical evidence; a future
  rerun under a different commit naturally gets a different fingerprint and does not resume
  an old checkpoint under the new identity (fingerprint-bound resume unchanged).

## 6. Failure and recovery review

- Missing `CODEGG_R001_IMPLEMENTATION_COMMIT` fails fast in the harness (`env::var` expect)
  before encoder load; empty/non-hex/bad-length values fail fast in `run_r001_sweep` via
  `validate_implementation_commit` before fixture checks complete and before any encoder
  load or sweep measurement.
- No implicit fallback: there is no code path that shells out to the repository; the sweep
  takes provenance only from its explicit argument (covered by
  `sweep_rejects_invalid_provenance_before_encoder_work`).
- Checkpoint/resume semantics unchanged apart from the provenance input now being explicit;
  fingerprint mismatch still recomputes instead of resuming stale receipts.

## 7. Migration and compatibility review

No user-facing schema, protocol, storage, or runtime migration. The only signature change is
the experiment-only `run_r001_sweep(output_dir, implementation_commit)`; its single internal
test call site was updated atomically. Historical checkpoints stay historical; future reruns
fingerprint under their own explicit commit.

## 8. Security review

No authorization, secret, path-validation, or privilege-boundary change. The subprocess
removal shrinks the process-execution surface. No new denial-of-service bound is introduced:
validation is a constant-time string check before heavy work, and invalid input aborts before
resource-intensive stages.

## 9. Documentation and operations

- `PREREG_SWEEP_COMMAND` and module/test docs now record the env-var invocation.
- Planning docs reconciled: experiment roadmap `Status: closed`, duplicate M002 row removed;
  corrective addendum closed (see §12); registry C001 closed (see §12).
- Operator recovery: set `CODEGG_R001_IMPLEMENTATION_COMMIT` to the 40-char (or 64-char)
  hex SHA under test; the harness reports a commit-named error otherwise.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted run `35861891751` (closure commit) first attempt failed on `tests/interactive_process_sessions.rs::environment_overrides_apply_while_denied_vars_stay_stripped` (PTY `env` output missing the override) | None on C001: unrelated interactive-process timing flake; the file under test is untouched by this corrective, and the implementation run `35859251981` passed the same suite | Classified, no scope widened: `--failed` rerun of `35861891751` is green; no follow-up registered |

All four plan defects (D1–D4) are closed. No new defect was found. Hosted CI exposed no
related failure and no unrelated substantive failure.

## 11. Roadmap disposition

- Corrective C001: **closed**. Repository-clean closure restored without changing the
  retrieval experiment's technical result.
- Retrieval-architecture workstream: **remains closed** with the M003 negative recall verdict
  (ceiling-proven; no operating point, no v4 plan) as the controlling technical result.
- Order-invariance M005: **remains blocked** (hard-blocked on a positive M004; historical
  evidence; not unblocked by this workstream — see dependency audit below).
- Live-primary-model M004: **remains blocked**.
- This corrective **unblocks nothing**: the next substantive advisor work, if pursued,
  requires a separate retrieval-signal experiment.

Dependency audit (registry `Blocked work` + affected roadmap graphs): the just-closed C001
appears in no registered plan's hard/interface dependency list as a predecessor. The only
registered blocked advisor item, order-invariance M005
(`plans/implementation/tool-selection-advisor-order-invariance-experiment/005-fresh-v4-preregistered-qualification.md`),
is hard-blocked on a positive order-invariance M004, which C001 does not change (M004 stays
closed negatively). No registered plan moves from `blocked` to `ready` in this commit.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps: retrieval-architecture closure corrective
  `active` → `closed` (C001 closed; current milestone updated to closure record).
- `plans/registry.md` Dependency-ready implementation plans: C001
  `plans/implementation/tool-selection-advisor-retrieval-architecture-closure-corrective/001-execution-ownership-and-planning-closure.md`
  `active` → `closed` (closure at this file; implementation `220d3638`; hosted run `35859251981`).
- `plans/registry.md` closure-corrective gate paragraph: C001 `active` → closed with
  implementation commit + closure record + hosted CI run.
- `plans/registry.md` Recently closed work: C001 row added.
- `plans/subsystems/tool-selection-advisor-retrieval-architecture-closure-corrective-addendum.md`:
  `Status: active` → `closed` (C001 closed).
- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md`:
  already reconciled in the implementation commit (`Status: closed`, duplicate M002 row
  removed); confirmed here.
- `plans/implementation/tool-selection-advisor-retrieval-architecture-closure-corrective/001-execution-ownership-and-planning-closure.md`:
  `Status: active` → `implemented` (evidence gathered; see this closure).
