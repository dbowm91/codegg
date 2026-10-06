# Decision-Model Extraction and Runtime Post-Closure Corrective C001 — Final Hosted CI and Merge Qualification

Status: ready for handoff

Repository baseline: `291a361d402c7062a713278e8e5ef3a840cee884`

Source corrective addendum:

- `plans/subsystems/decision-model-extraction-runtime-post-closure-merge-corrective-addendum.md`

Historical predecessor:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md` — M001-M006 closed.
- `plans/closure/decision-model-extraction-runtime/006-status.md` — final implementation closure before merge qualification.

Applicable ADRs:

- `plans/adrs/ADR-0013-external-decision-model-training-and-runtime-boundary.md`
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: polish / verification

Hard dependencies:

- M001-M006 closed — satisfied.
- No product/runtime dependency.

## 1. Objective

Turn the locally closed decision-model extraction/runtime branch into a merge-qualified change against current `main`.

C001 owns exactly four things:

1. reconcile the branch with current `main` without changing the accepted architecture;
2. perform a final local ownership/compatibility verification;
3. open/finalize the pull request and obtain required hosted CI on the exact final head;
4. record a closure verdict that either authorizes merge or names a concrete corrective blocker.

C001 is not a seventh feature milestone.

## 2. Current evidence and trigger

At the planning baseline:

- branch: `codex/decision-model-extraction-runtime`;
- head: `291a361d402c7062a713278e8e5ef3a840cee884`;
- compared with `main`: ahead with the full M001-M006 implementation;
- no pull request exists for the branch;
- GitHub reports no workflow runs or combined commit statuses for the head;
- M006 records successful local workspace/supported-feature/Clippy/compatibility evidence but explicitly no new hosted CI run;
- SDM public `main` is the CodeGG runtime pin `8139b064bdcf3212e8f6fd912e801a479b55751c` at planning time.

Therefore the remaining defect is integration evidence, not missing product functionality.

## 3. Invariants that cannot regress

- default CodeGG remains model-free and network-silent with decision backend off;
- SDM remains opt-in through `decision-runtime-sdm`;
- `sdm-training` does not enter CodeGG's runtime graph;
- candidate authority remains CodeGG-owned and bounded by `ResolvedToolSurface`;
- decision backends cannot invent candidate/tool authority;
- promotion remains unavailable to unqualified artifacts;
- System One Rank remains explicit unsupported;
- causal-frontier policy and capture/export consent remain independent;
- no implicit model download;
- historical M001-M006 records/assets stay immutable;
- future generic training/runtime implementation remains SDM-owned.

## 4. Work package A — Reconcile with current main

### Required actions

1. Fetch/inspect current `main` and compare it to the C001 branch head.
2. If `main` has advanced, integrate it using the repository's normal branch policy.
3. Resolve conflicts semantically:
   - preserve newer unrelated `main` behavior;
   - preserve decision-runtime ownership and safety invariants;
   - avoid reverting newer planning/CI corrections from `main`.
4. Inspect the post-resolution diff for accidental scope expansion.

### Acceptance evidence

- exact pre/post head and base SHAs;
- conflict list, if any;
- explanation for every semantic conflict resolution;
- no unresolved merge markers;
- `git diff --check` green;
- focused tests for any conflicted production surface.

### Stop condition

If current `main` changed a decision/tool-authority/provider/runtime contract such that the branch cannot be reconciled mechanically while preserving ADR-0013, stop and write a bounded corrective plan rather than redesigning inside C001.

## 5. Work package B — Final local ownership and dependency audit

### Required actions

Verify the final branch contains the intended ownership split:

CodeGG retains:

- `crates/codegg-core/src/decision.rs`;
- CodeGG backend adapters;
- tool candidate/context projection;
- deterministic fallback/preselection;
- promotion/disclosure policy;
- causal-frontier policy;
- training-data capture/export/consent;
- operator configuration/status.

SDM owns:

- generic training/evaluation;
- reusable local artifact runtime;
- generic artifact production.

Verify retired code/dependencies have not returned:

- Candle stack absent;
- legacy advisor training/encoder features absent;
- architecture-specific train/eval CLI absent;
- `sdm-training` absent from runtime graph.

### Required commands/evidence

Record the outputs relevant to:

```bash
cargo tree --locked -e normal
cargo tree --locked --features decision-runtime-sdm -e normal
rg -n 'candle|tool-advisor-training|tool-advisor-encoder' Cargo.toml Cargo.lock src crates
rg -n 'sdm-training' Cargo.toml Cargo.lock
```

Interpret hits rather than requiring a naïve zero-result if historical documentation/comments legitimately contain the terms.

## 6. Work package C — Final local correctness verification

Run, at minimum:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked --profile ci
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci
cargo check --locked -p codegg --features decision-runtime-sdm
scripts/check_sdm_compatibility.sh <exact-sdm-checkout> 8139b064bdcf3212e8f6fd912e801a479b55751c
scripts/verify.sh quick
git diff --check
```

Also run focused tests covering:

- decision request/response validation;
- SDM artifact tamper/version mismatch;
- decision adapter candidate identity rejection;
- pre-turn decision ranking;
- tool-search decision ranking;
- promotion qualification fallback;
- System One off/no-network behavior;
- System One Rank unsupported;
- causal-frontier regression;
- capture/export consent;
- CLI `tool-advisor status --json`.

If test target names moved during conflict resolution, use their current equivalents and record the mapping.

## 7. Work package D — SDM pin qualification

### Required actions

1. Verify the exact CodeGG pin in `Cargo.toml`.
2. Verify the pinned commit exists in `dbowm91/sdm`.
3. Check out exactly that commit for the compatibility runner.
4. Run the frozen compatibility suite.
5. Record artifact digest, runtime revision, fixture count, and request execution count.
6. Verify runtime dependency graph excludes training.

### Acceptance

The final branch must reproduce the accepted compatibility contract without changing frozen fixtures merely to make the external runtime pass.

If SDM `main` is newer than the pin, leave the CodeGG pin unchanged unless there is an independent reason to upgrade. C001 is merge qualification, not dependency advancement.

## 8. Work package E — Pull request and hosted CI

### Pull request

Open a PR from `codex/decision-model-extraction-runtime` to `main`.

The PR body should summarize:

- ADR-0013 ownership change;
- M001-M006 status;
- external SDM repository/pin;
- removal of legacy training/runtime/Candle stack;
- backend-neutral `DecisionEngine`;
- local SDM and System One backend behavior;
- default-off/unqualified promotion posture;
- local verification;
- C001 requirement for hosted qualification.

Do not represent the SDM smoke artifact as a qualified production model.

### Hosted checks

Wait for/run the repository-required PR workflows and record all required conclusions.

At minimum:

- root `CI / verify` must be SUCCESS on the exact final head.

Also require any other workflow triggered by the final diff according to repository rules. Do not manually exempt a red required job because local tests passed.

### Rerun policy

For any red job:

- capture job/step/log evidence;
- classify root cause;
- if no source change is needed and evidence supports a transient runner/test-environment failure, a same-SHA rerun is permitted and must be recorded;
- if any repository change is required, push the fix and require a fresh complete hosted run on the new SHA.

## 9. Work package F — Final closure and merge recommendation

Create:

- `plans/closure/decision-model-extraction-runtime-post-closure-merge-corrective/001-status.md`

The closure record must include:

- final implementation head SHA;
- final `main` base SHA;
- PR number;
- hosted workflow run ids and conclusions;
- local verification table;
- SDM pin/digest/compatibility evidence;
- diff/ownership audit;
- unresolved findings by severity;
- explicit disposition:
  - **closed — merge recommended**, or
  - **corrective required**, or
  - **blocked**.

Only **closed — merge recommended** authorizes the branch to merge.

After merge, either append a small post-merge evidence subsection to the C001 closure record if repository convention permits current-authority closure records to record the merge commit, or add the merge commit/PR result to the registry without rewriting M001-M006 history.

## 10. Documentation and registry changes

During C001 implementation:

- update this corrective addendum status;
- update `plans/registry.md`;
- ensure `plans/subsystems/decision-model-extraction-runtime-roadmap.md` remains closed and contains no stale blocked/active milestone prose;
- do not reopen M006;
- do not move generic future model research back into CodeGG planning.

If the PR changes only evidence/docs after local verification, hosted CI still must qualify that exact final PR head.

## 11. Acceptance criteria

C001 closes only when:

1. branch is reconciled with current `main`;
2. local canonical verification is green;
3. SDM pin and compatibility are reproduced;
4. runtime dependency graph remains training/ML-framework free;
5. PR exists against `main`;
6. required hosted checks are green on the exact final head;
7. no unresolved medium/high finding remains;
8. planning state is internally consistent;
9. closure record explicitly recommends merge.

## 12. Stop conditions

Stop and write a new corrective plan if:

- hosted CI finds a production correctness/security defect requiring more than a narrow merge fix;
- mainline drift invalidates ADR-0013 assumptions;
- the SDM pin no longer reproduces the compatibility contract;
- a conflict requires changing public decision semantics;
- a remote backend becomes required/default-on;
- candidate authority or promotion qualification would be widened;
- fixing CI would require weakening a guard/test rather than correcting behavior.

## 13. Closure evidence required

- branch/base/final head SHAs;
- PR URL/number;
- hosted run ids/job conclusions;
- local command results/test counts;
- conflict-resolution record;
- Cargo/dependency ownership evidence;
- SDM pin, artifact digest, fixture/request counts;
- no-network/default-off evidence;
- authority/promotion negative evidence;
- unresolved findings table;
- final merge recommendation.
