# Tool-Selection Advisor Sequence-Encoder Experiment P001 — Evidence-State and Preregistration Polish

Status: active

Repository baseline: `8487967d9cc2605c398d3c608e353c8871289a7d`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#p001--evidence-state-and-preregistration-polish`

Primary class: polish/evidence integrity.

## Objective

Clean the planning/documentation state before a new model experiment and strengthen preregistration discipline without rewriting historical closure records.

## Current defects

1. `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md` top/status table says C004 is closed, but its C004 subsection still says `Status: ready`.
2. `architecture/tool-advisor-framework-spike.md` still describes the repository-local hashed contextual scorer as the selected architecture without recording the later C004 disposition B.
3. The C004 closure records hosted CI as unavailable, but GitHub Actions run `35628654554` subsequently completed successfully; job `verify` concluded `success`.
4. The C004 preregistration manifest and final result landed in the same commit. The harness remained deterministic and the negative result is credible, but future positive qualification should have an independently auditable preregistration commit before final-test evaluation.
5. The registry still labels the first post-closure advisor workstream `active` although its only remaining milestone is blocked on a new architecture experiment.

## Required changes

- Do not edit the historical C004 closure to hide its original timing.
- Add a supplemental evidence note under `plans/closure/tool-selection-advisor-evidence-integrity-corrective/` recording the successful hosted CI run and timestamp.
- Correct the stale C004 roadmap subsection to `closed (disposition B)`.
- Update `architecture/tool-advisor-framework-spike.md` to clearly distinguish:
  - historical M002 selection;
  - C004 demotion of `contextual-embedding-v2` to research/observe baseline;
  - no currently qualified learned architecture.
- Change registry status of `Tool-selection advisor post-closure corrective` from active to blocked while live M004 has no positive offline model.
- Document a two-commit qualification protocol for future model experiments:
  1. commit/freeze preregistration manifest and pass CI;
  2. only afterward run final test/family-holdout evaluation and commit results/closure.
- Add preregistration provenance fields to future protocol schema/output:
  - protocol hash;
  - dataset/split fingerprints;
  - model/config hashes;
  - declared gate set;
  - preregistration commit SHA supplied by the operator/closure process.
- The qualification harness should echo these values and fail on manifest drift, but it does not need to shell out to Git or become a source-control policy engine.

## Verification

- documentation cross-check finds no C004 `ready` state;
- registry has one truthful blocked live-M004 state;
- supplemental CI evidence points to run `35628654554`, job `verify`, conclusion `success`;
- preregistration manifest/result schema tests remain deterministic;
- `scripts/verify.sh quick`;
- `git diff --check`.

## Acceptance

P001 closes when current planning state is internally consistent, successful hosted CI is recorded additively, and the next qualification campaign has an explicit separate-commit preregistration contract.
