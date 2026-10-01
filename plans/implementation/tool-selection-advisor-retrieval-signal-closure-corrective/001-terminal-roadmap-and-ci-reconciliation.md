# Tool-Selection Advisor Retrieval-Signal Closure Corrective C001 — Terminal Roadmap and CI Reconciliation

Status: ready for handoff

Repository baseline: `a9f65b56089c7b521810dc2f5c26ffafd729cbf7`

Source corrective:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-closure-corrective-addendum.md`

Predecessor closures:

- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001r-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/002-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/003-status.md`

Controlling architecture/process:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`
- `plans/003-planning-process.md#7-corrective-passes`

Primary class: closure/correctness cleanup.

## 1. Objective

Make the retrieval-signal experiment repository-clean after its terminal negative M003 result.

This pass owns exactly two things:

1. planning-state reconciliation;
2. classification and, if owned, correction of hosted default-feature Clippy failure `36866669727`.

No retrieval/model experiment occurs in this milestone.

## 2. Current evidence

The final technical state is already determined:

- corrected dev retrieval target: 53 inferable positives;
- M002 best deterministic mode: normalized union, 51/53 = 0.9623;
- M002 fails u64 >=0.99 and u128 >=0.98, passes u256 >=0.95;
- M003 evaluates 288 preregistered frozen-MiniLM projection arms;
- 0/288 clear gates;
- best projected u64/u128 = 0.9623, tying deterministic union;
- best projected u256 = 0.9057, below deterministic union;
- unknown/renamed and name-masked generalization guards fail materially;
- no projection artifact frozen;
- authority violations remain zero.

These findings are immutable inputs to C001.

## 3. Work package A — reproduce hosted Clippy

Run the exact routine-CI command under current stable Rust:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Also record:

```bash
rustc --version --verbose
cargo --version
git rev-parse HEAD
```

If the failure reproduces, capture:

- crate;
- file/line;
- lint;
- compiler/toolchain version;
- whether the offending code is default-feature compiled;
- earliest introducing commit using repository history where practical.

Do not use `--all-features` as a substitute for reproducing the routine CI command.

## 4. Work package B — ownership classification

### Owned by retrieval-signal line

If the failing code was introduced by:

- M002 implementation `eff890ec`,
- M003 implementation `2df9a5f7`,
- or a follow-up in the same advisor path,

fix the smallest source issue and add a regression/static check when one is meaningful.

Do not alter experimental math/results to satisfy lint.

### Unrelated

If the failure originates elsewhere:

- identify exact owning subsystem/commit;
- verify whether a plan/corrective already owns it;
- add a concise dependency note to this closure;
- do not edit unrelated production code in this corrective.

If no owner exists and main cannot become green without it, register a separate corrective rather than widening C001.

## 5. Work package C — terminal roadmap reconciliation

Update:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`

Required final state:

```text
Status: closed (negative)
```

Rewrite the dependency graph/current-state bullets so they describe completed history rather than preregistration-era readiness.

Required milestone state:

- M001 — blocked/closed hard stop;
- C001 evaluation-semantics corrective — closed B;
- M001R — closed positive;
- M002 — closed negative-but-valid;
- M003 — closed negative;
- M004 — blocked/not reached; no eligible retrieval candidate;
- M005 — blocked/not reached; no positive M004.

The roadmap should state that any future architecture attempt is a separate workstream, not M006.

Do not remove historical reasoning/hypotheses; mark them as evaluated where appropriate.

## 6. Work package D — registry reconciliation

Update `plans/registry.md`:

- retrieval-signal subsystem row: active → closed negative;
- closure corrective row: active while C001 executes, then closed;
- C001 implementation row: ready → closed after evidence;
- blocked-work row: M004/M005 terminal blockers remain explicit;
- remove stale readiness language;
- add recent-closure evidence for M003/C001 as appropriate;
- preserve live-primary-model block.

Keep the registry compact; do not duplicate M002/M003 closure tables.

## 7. Verification

Required local commands:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
git diff --check
```

If C001 changes any advisor source code, also run focused tests for the touched module.

Do not require encoder-training feature tests unless source under that feature changes.

## 8. Hosted CI

Push the implementation/reconciliation commit and require ordinary `CI / verify`.

Record:

- run id;
- commit SHA;
- Workspace Clippy result;
- workspace-test result;
- whether live Eggwork was included;
- final conclusion.

If hosted CI is still red for a proven unrelated registered blocker, C001 may be conditionally closed only if planning governance permits and the exact blocker is named. Prefer green main.

## 9. Closure record

Create:

- `plans/closure/tool-selection-advisor-retrieval-signal-closure-corrective/001-status.md`

It must include:

- implementation commits;
- exact CI failure classification;
- source fix if any;
- planning files reconciled;
- verification commands/results;
- hosted run;
- final statement that M004/M005 remain blocked and no model/retrieval work was reopened.

## 10. Stop conditions

Stop and register separate work if:

- CI failure belongs to a different subsystem and requires nontrivial production changes;
- fixing Clippy would change retrieval/model behavior;
- new evidence calls M002/M003 result correctness into question;
- roadmap reconciliation exposes a new architecture decision rather than stale status.

## 11. Acceptance

C001 closes only when terminal negative state is unambiguous, the current CI failure is properly owned/classified, and repository verification is trustworthy.

Nothing in C001 authorizes a new retrieval architecture.
