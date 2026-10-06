# Decision-Model Extraction and Runtime Milestone 006 — Legacy Training/Runtime Retirement

Status: closed

Repository baseline: `533be5941ac48334743de555cda96d8695cc6527`

Source roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m006--legacy-trainingmodel-code-retirement-and-qualification`

Applicable ADRs:

- `plans/adrs/ADR-0013-external-decision-model-training-and-runtime-boundary.md`
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` for retained historical/security invariants.

Primary class: polish

Hard dependency:

- M005 closed; closure record includes the fresh source/dependency reachability inventory and M001 fixture compatibility evidence.

## 1. Objective

Complete the ownership migration by removing generic training/framework/model-specific runtime implementation from CodeGG where it no longer has a production or required evidence owner, retiring obsolete advisor training/encoder feature flags and dependencies, reconciling stale planning/docs, and qualifying the final no-backend/local-backend/external-backend failure posture.

The goal is a smaller CodeGG maintenance surface, not deletion for its own sake.

## 2. Preconditions

Before deletion, produce a fresh inventory proving:

- all production learned inference routes through `DecisionEngine`;
- the external repository owns reusable training/evaluation/artifact/runtime implementation;
- M001 compatibility fixtures remain runnable;
- historical closure records identify immutable implementation commits;
- no other CodeGG subsystem uses Candle or candidate training modules;
- causal-frontier files are distinguished from generic learned-model files.

Anything with a live owner remains.

## 3. Likely retirement candidates

Subject to the fresh inventory, expected candidates include model/training-specific portions of:

- `src/tool_advisor/training.rs`;
- sequence encoder/ranking/qualification experiment code;
- generic retrieval projection/late-interaction experiment implementations;
- contextual learned-model training/runtime implementation;
- requalification code that exists solely for frozen historical architecture runs;
- scripts used only to generate already-frozen learned-model holdouts/artifacts;
- advisor training/encoder-only feature flags;
- optional Candle dependencies if no remaining consumer exists.

This list is not an instruction to delete every named file. Mixed CodeGG-owned policy/fixtures must be split or retained.

Expected retained areas include:

- tool candidate/context adapter;
- `DecisionEngine` integration;
- deterministic BM25/discovery fallback;
- generic backend diagnostics/configuration;
- local/external backend adapters;
- training-event export/capture if still a supported user capability;
- causal-frontier implementation and tests;
- historical assets/closures needed for traceability.

## 4. Invariants that must not regress

- Git history and closure records remain sufficient to inspect the implementation used for historical experiment verdicts.
- Frozen result/receipt/preregistration assets are not rewritten.
- No current production behavior depends on code removed in this milestone.
- Default/no-backend CodeGG remains fully functional.
- Local runtime backend remains functional through the external dependency.
- System One/off failure posture remains bounded.
- Causal-frontier active/observe modes remain intact.
- Removing features must produce clear compile/config errors or compatibility diagnostics, not silently reinterpret old settings.
- MSRV/support platform contracts are preserved.

## 5. Scope

### In scope

- source/dependency/feature inventory;
- remove dead generic learned-model training/runtime implementation;
- remove or rename obsolete CLI commands;
- remove training/encoder-only Cargo features;
- remove Candle dependencies if unused;
- config compatibility cleanup/deprecation handling;
- archive/reconcile architecture docs;
- terminalize stale tool-advisor experiment planning states;
- final dependency and binary-size comparison;
- broad CI/qualification.

### Explicitly out of scope

- Removing historical closure/receipt assets merely to save repository size.
- Re-running frozen final evaluations.
- Deleting causal-frontier code.
- Removing generic local capture/export if it remains useful to feed the external trainer.
- Choosing/bundling a default model.
- Changing agent tool policy/disclosure semantics.
- New training architecture.

## 6. Required production changes

### Cargo/features

Audit root/workspace manifests.

Expected retirement targets, if no remaining owner:

- `tool-advisor-training`;
- `tool-advisor-encoder-experiment`;
- `tool-advisor-encoder-training`;
- `candle-core`, `candle-nn`, `candle-transformers`.

Whether `tool-advisor` itself remains should be decided by its new meaning. Prefer a backend/integration feature name if it no longer denotes the historical model stack; provide a bounded alias/deprecation if necessary rather than retaining misleading semantics indefinitely.

### CLI

Remove architecture-specific train/eval/probe/sweep commands with no current owner.

Retain user-facing generic operations that still make sense, such as backend status/inspect, local artifact inspect, or explicit dataset export.

For removed commands, update help/tests/docs together.

### Source

Delete only modules proven unreachable from supported production/test paths except historical reproducibility.

If keeping a legacy module solely for archival reproducibility would continue to impose heavy dependencies/build costs, prefer Git history + immutable closure commits over compiling it forever.

### Assets

Classify assets:

- historical evidence: retain;
- compatibility fixtures: retain;
- redistributable model weights: follow external repository ownership/license policy;
- generated scratch/reference assets not committed: document external workflow.

Do not change hashes in old closure records.

### Planning reconciliation

Update historical roadmaps/registry so future learned-model work points to the external decision-model project rather than adding another architecture experiment under CodeGG.

In particular, reconcile terminal blocked successors such as order-invariance M005 and any stale top-level `Status: active` that exists only because a prerequisite closed negatively.

Do not rewrite old milestone dispositions.

## 7. Ordered work packages

### A — Reachability/dependency inventory

Use code search, Cargo feature/dependency graph, CLI registration, tests, and planning references to classify each legacy component.

Acceptance: deletion set has an explicit owner/reason; no guessed dead code.

### B — Remove training/model framework source

Delete/split only the proven legacy generic model implementation.

Acceptance: CodeGG still builds/tests under supported features using the external runtime.

### C — Feature/dependency cleanup

Remove obsolete feature flags/dependencies and update lockfile.

Acceptance: `cargo tree` proves training frameworks are absent from default and supported decision-runtime builds unless another named owner remains.

### D — CLI/config cleanup

Retire old commands/options with explicit compatibility messaging.

Acceptance: command/help snapshots and config parsing tests reflect the supported surface; stale settings fail safely.

### E — Documentation/planning reconciliation

Mark historical model research as historical, point new experimentation externally, and correct terminal stale roadmap/registry state.

Acceptance: registry contains no dependency-ready CodeGG plan for another learned architecture unless separately justified.

### F — Final qualification

Exercise:

- no backend;
- valid local artifact backend;
- missing/corrupt local artifact;
- System One backend off;
- System One configured but unavailable/timeout;
- unsupported Rank external backend;
- causal-frontier modes;
- telemetry/capture independence.

Acceptance: all fallbacks/authority invariants hold and workspace verification is green.

## 8. Failure and rollback

This milestone must be implemented in reviewable deletion groups so a mistakenly removed owner is attributable.

No user data migration is destructive. Config compatibility should prefer warning/fallback before hard removal unless a stale option would be unsafe.

If removing a legacy implementation reveals an undocumented production caller, stop and restore ownership classification rather than patching around it.

## 9. Security

Dependency removal must not weaken:

- artifact hash validation;
- remote endpoint credential/TLS policy;
- candidate authority validation;
- capture/telemetry consent;
- causal-frontier authority boundaries.

Removing old code should reduce rather than replace these controls.

## 10. Required verification

Before/after evidence:

```bash
cargo tree -e features
cargo metadata --locked --format-version 1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --locked -p codegg --all-targets --features server,plugins,lsp-test-support -- -D warnings
cargo nextest run --workspace --locked --profile ci
scripts/verify.sh quick
git diff --check
```

Do not run `--all-features`: repository instructions exclude it because it
enables real-server tests. The supported feature set above is the workspace
feature qualification surface.

Also run:

- M001 compatibility fixture suite;
- M003 local runtime tests;
- M004 fake-server suite;
- M005 agent-loop advisor tests;
- causal-frontier focused tests;
- CLI/help/config snapshot tests;
- release/default feature build used by CI.

Record binary/dependency deltas where meaningful.

## 11. Documentation updates

At minimum:

- `architecture/tool-advisor.md`
- `architecture/tool-advisor-framework-spike.md` — mark model-framework content historical; do not falsify dates/results.
- Cargo feature/dependency docs.
- advisor/decision backend operator docs.
- `plans/subsystems/decision-model-extraction-runtime-roadmap.md`
- `plans/registry.md`
- affected historical roadmap status lines where planning state is stale/terminal.

## 12. Acceptance criteria

M006 closes only when:

1. generic training/evaluation/model architecture implementation no longer has a CodeGG production owner;
2. the external repository is the documented owner;
3. obsolete training/encoder features and unowned ML dependencies are removed;
4. supported local/external/off decision paths pass;
5. causal-frontier and deterministic fallback remain intact;
6. historical closure/assets remain traceable;
7. stale learned-experiment planning is terminal/reconciled;
8. default CodeGG remains model-free and fully functional.

## 13. Stop conditions

Stop if:

- deletion breaks historical evidence attribution;
- a supposedly dead module has a supported production consumer;
- cleanup would require deleting causal-frontier/domain policy;
- external runtime cannot replace a removed runtime capability;
- config migration would silently alter authority or enable a network backend;
- dependency removal causes unsupported platform/MSRV regression not owned by this line.

## 14. Closure evidence required

- final source/dependency/feature inventory;
- deleted/retained component table with owners;
- Cargo tree/metadata before-after;
- CLI/config migration evidence;
- final backend/fallback qualification matrix;
- causal-frontier regression evidence;
- hosted CI/workspace verification;
- documentation/registry reconciliation;
- unresolved findings and successor ownership;
- final roadmap disposition.
