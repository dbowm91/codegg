# Decision-Model Extraction and Runtime — Post-Closure Merge Qualification Corrective Addendum

Status: active; C001 ready

Repository baseline reviewed: `291a361d402c7062a713278e8e5ef3a840cee884`

Predecessor roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md` — M001-M006 closed.
- `plans/closure/decision-model-extraction-runtime/001-status.md` through `006-status.md` — accepted historical closure evidence.

Accepted architecture:

- `plans/adrs/ADR-0013-external-decision-model-training-and-runtime-boundary.md`
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` — retained authority/fallback/consent invariants.

External dependency of record:

- `dbowm91/sdm`
- CodeGG runtime pin: `8139b064bdcf3212e8f6fd912e801a479b55751c`
- SDM license: MIT
- CodeGG compatibility fixture contract: `assets/decision-runtime/compatibility-v1.jsonl`

Canonical planning reference:

- `plans/003-planning-process.md` §7, corrective passes.

Primary class: polish / merge qualification.

## 1. Why this corrective exists

M001-M006 are technically closed on the implementation branch and the final M006 local qualification is strong:

- default workspace nextest: 12,282 passed / 5 skipped;
- supported-feature nextest: 9,426 passed / 5 skipped;
- workspace Clippy: green;
- `scripts/verify.sh quick`: green;
- SDM compatibility: 12 fixtures validated / 11 valid runtime requests executed;
- legacy Candle/training/runtime implementation retired;
- decision-runtime roadmap and registry marked closed.

The branch is nevertheless not yet integration-closed relative to `main`:

1. `codex/decision-model-extraction-runtime` is substantially ahead of `main` and has no pull request.
2. GitHub reports no hosted workflow runs or commit statuses for the current branch head.
3. M006 explicitly records local verification only and says no new hosted CI run was started.
4. The roadmap had one stale M006 prose status that still said `blocked on M005` after the roadmap/table/registry were already closed. That planning inconsistency is corrected as part of registering this addendum.
5. The merge is large enough that final integration evidence must be collected on the exact PR head rather than inferred from local closure evidence.

The historical M001-M006 closure records remain valid and MUST NOT be rewritten. C001 owns only final hosted integration, branch/PR reconciliation, and merge-readiness evidence.

## 2. Corrective milestone

### C001 — Final hosted CI and merge qualification

Status: ready.

Implementation plan:

- `plans/implementation/decision-model-extraction-runtime-post-closure-merge-corrective/001-final-hosted-ci-and-merge-qualification.md`

Objective:

Prepare the completed decision-runtime branch for merge to `main`, prove the exact final PR head on hosted CI, reconcile any merge/base drift without reopening the architecture, and produce one post-closure record that authorizes merge if and only if the final integrated revision remains green.

## 3. Invariants

C001 must preserve:

- M001-M006 historical closure and dispositions;
- ADR-0013 ownership: SDM owns generic training/runtime; CodeGG owns bounded decision contract, adapters, candidate authority, deterministic fallback, causal-frontier policy, consent, and actuation;
- CodeGG default operation with no model/runtime/network dependency;
- SDM runtime remains opt-in and unqualified for promotion;
- `ResolvedToolSurface` remains the candidate authority ceiling;
- System One remains explicit opt-in and does not synthesize Rank from Choice;
- no implicit model download;
- causal-frontier behavior remains CodeGG-owned and independent;
- frozen historical advisor assets and qualification results remain immutable;
- the exact SDM revision and compatibility fixture contract remain reproducible.

## 4. Non-goals

C001 does not authorize:

- new model training or architecture experiments;
- qualifying the smoke SDM artifact for promotion;
- changing decision semantics;
- changing the System One API mapping;
- changing candidate retrieval/promotion policy;
- deleting additional historical assets merely for cleanup;
- new Cargo dependency upgrades unrelated to merge conflict resolution;
- changing the SDM pin unless the current pin can no longer be reproduced or merged safely;
- rewriting M001-M006 closure records;
- broad refactoring discovered opportunistically during PR preparation.

Any material production defect found by hosted CI requires a new corrective plan or an explicit C001 scope amendment before implementation.

## 5. Merge qualification requirements

The exact candidate revision intended for merge must satisfy all of the following:

1. branch is rebased/merged against the then-current `main` with no unresolved semantic conflict;
2. final diff remains within the ADR-0013 ownership boundary;
3. root hosted `CI / verify` succeeds on the exact PR head;
4. every other path-triggered required workflow for the final diff succeeds, or its non-trigger is explicitly justified;
5. SDM compatibility check passes against the exact pinned full SHA;
6. default and supported-feature dependency graphs contain no retired Candle/training stack;
7. no default network attempt or decision backend activation is introduced by conflict resolution;
8. no new unqualified backend acquires promotion authority;
9. planning registry/roadmap/closure references remain mutually consistent;
10. the PR contains the complete M001-M006 implementation + closure history and this C001 corrective.

A locally green branch without hosted CI is not sufficient closure evidence.

## 6. Base-drift and conflict policy

Before opening or finalizing the PR:

- compare `main...codex/decision-model-extraction-runtime`;
- identify commits landed on `main` since the original `533be594...` implementation baseline;
- classify conflicts by ownership rather than mechanically preferring branch or main;
- preserve newer `main` behavior outside the decision-runtime workstream;
- preserve C001/M001-M006 invariants for decision-runtime-owned files;
- rerun focused tests after any semantic conflict resolution.

If `main` advances after a green hosted run and the merge operation produces a different tree, update the branch and rerun required hosted checks on the new exact head. Do not treat an obsolete green SHA as merge evidence.

## 7. Hosted CI evidence

At minimum C001 requires the repository's canonical pull-request root verification workflow on the final PR head.

The closure record must capture:

- PR number;
- exact head SHA;
- base SHA used for qualification;
- workflow run id(s);
- conclusion for each required job;
- full workspace test counts where emitted;
- any reruns, with the original failure classification and same/different SHA;
- whether failures were product defects, test defects, environment flakes, or unrelated base failures.

A rerun may satisfy closure only when:

- the source SHA is unchanged;
- the failure is classified with evidence;
- no production/test patch was required to make the rerun pass.

If code changes are required, the new SHA must receive a fresh hosted run.

## 8. SDM pin and cross-repository verification

On the final branch head:

- verify `Cargo.toml` pins `sdm-core` and `sdm-runtime` to the expected immutable full SHA;
- verify that SHA exists in `dbowm91/sdm`;
- run `scripts/check_sdm_compatibility.sh` against an exact checkout of that SHA;
- record 12-fixture / 11-runtime-request compatibility or the current exact contract count if legitimately changed by a separately approved contract revision;
- verify runtime dependency tree excludes `sdm-training` and retired ML frameworks.

Do not advance the SDM pin just because `sdm/main` moved. A pin change requires a reason and fresh compatibility/resource evidence.

## 9. Final local verification before hosted handoff

Run the canonical repository-supported equivalents of:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked --profile ci
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci
cargo check --locked -p codegg --features decision-runtime-sdm
scripts/check_sdm_compatibility.sh <sdm-checkout> 8139b064bdcf3212e8f6fd912e801a479b55751c
scripts/verify.sh quick
git diff --check
```

Respect repository guidance that forbids inappropriate `--all-features` workspace sweeps.

If the host requires platform-specific linker flags, record them as environment evidence rather than changing repository build semantics solely for the local machine.

## 10. Final diff/ownership audit

Before merge, inspect the final diff and confirm:

- no generic training implementation remains in CodeGG;
- no Candle dependencies/features have returned;
- no CodeGG authority/policy moved into SDM;
- no default-on decision backend/network path was introduced;
- System One Rank remains unsupported unless separately planned;
- `training_data.rs` remains only for supported capture/export/consent ownership;
- causal-frontier modules remain intact;
- historical evidence files were not silently rewritten;
- obsolete architecture-specific CLI/train/eval commands remain retired;
- `tool-advisor status --json` still reports backend/policy qualification truthfully without inference.

## 11. Documentation and planning reconciliation

C001 may update current-authority planning/documentation only.

Required:

- this addendum;
- C001 implementation plan;
- `plans/registry.md`;
- stale current-status prose in the decision-runtime roadmap;
- C001 closure record after hosted qualification.

Do not edit historical M001-M006 closure records except to correct a literal broken link/hash that makes the record unusable; such a correction must be explicitly documented.

After C001 closes and the PR merges, the registry should show:

- decision-model extraction/runtime: closed;
- post-closure merge corrective: closed;
- no ready/blocked successor in CodeGG for generic model architecture/training;
- SDM as the owner for future generic small-decision-model experimentation.

## 12. Completion definition

C001 closes when:

- the final candidate is reconciled with current `main`;
- a PR exists against `main`;
- required hosted workflows are green on the exact final head;
- SDM compatibility and dependency boundaries remain green;
- no medium/high correctness, security, migration, or authority findings remain;
- current planning state is internally consistent;
- the closure record explicitly recommends merge and records the qualified head/base/run ids.

C001 closure authorizes merge. It does not itself claim that the merge has occurred unless the closure record is written after merge.

## 13. Status table

| Corrective | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 Final hosted CI and merge qualification | ready | `plans/implementation/decision-model-extraction-runtime-post-closure-merge-corrective/001-final-hosted-ci-and-merge-qualification.md` | pending | None; M001-M006 are closed. |
