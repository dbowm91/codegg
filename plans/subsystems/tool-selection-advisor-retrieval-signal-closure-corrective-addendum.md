# Tool-Selection Advisor Retrieval-Signal Closure Corrective Addendum

Status: closed

Closure:

- `plans/closure/tool-selection-advisor-retrieval-signal-closure-corrective/001-status.md`

Repository planning baseline: `a9f65b56089c7b521810dc2f5c26ffafd729cbf7`

Controlling architecture/process:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`
- `plans/003-planning-process.md#7-corrective-passes`

Predecessor evidence:

- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001r-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/002-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/003-status.md`
- hosted CI run `36866669727` on `7b509fafa079ea992f0320bc56b7a8abb19d18f2` — failed at Workspace Clippy.

## 1. Purpose

The retrieval-signal experiment has reached a valid terminal negative result:

- M001 hard-stopped on evaluation semantics;
- the evaluation-semantics corrective closed with disposition B;
- M001R closed positively;
- M002 closed negative-but-valid;
- M003 closed negatively with 0/288 projection arms clearing the frozen gates;
- no retrieval/projection artifact is eligible for M004;
- M004/M005 remain blocked.

Two closure-hygiene defects remain:

1. the subsystem roadmap still contains stale dependency prose describing C001 as the only ready handoff and M001R/M002 as blocked even though those steps are already closed;
2. hosted CI run `36866669727` is red at the default-feature Workspace Clippy step, while the M003 closure contains only local Clippy evidence and explicitly claims no hosted CI.

This corrective reconciles terminal planning state and classifies the CI failure without reopening the experiment.

## 2. Corrective scope

One milestone:

- `plans/implementation/tool-selection-advisor-retrieval-signal-closure-corrective/001-terminal-roadmap-and-ci-reconciliation.md`

Status: ready.

C001 must:

- reproduce/classify the hosted default-feature Clippy failure;
- fix it only if the failure is owned by changes in this retrieval-signal line;
- otherwise record the unrelated owner/control point without absorbing that work;
- make the retrieval-signal roadmap terminal/closed-negative;
- reconcile dependency graph, milestone text, registry, and blocked-work wording;
- obtain green hosted CI on the corrective implementation head unless the failure is proven unrelated and owned by an already-registered blocking workstream;
- add an additive closure record.

## 3. Invariants

This corrective MUST NOT:

- change retrieval scoring, representation, fusion, K, gates, labels, or candidate universes;
- retrain or fine-tune any model;
- freeze a projection artifact;
- reinterpret M002/M003 as positive;
- consume v4;
- unblock M004/M005;
- rewrite historical closure records;
- alter `ResolvedToolSurface` authority or advisor default-off/local-only behavior.

## 4. Terminal experiment state

After positive corrective closure, the retrieval-signal experiment should be represented as:

- workstream: **closed (negative)**;
- M001: blocked/closed hard stop;
- evaluation corrective C001: closed disposition B;
- M001R: closed positive;
- M002: closed negative-but-valid;
- M003: closed negative;
- M004: blocked/not reached because neither M002 nor M003 produced an eligible retrieval candidate;
- M005: blocked/not reached because M004 never opened.

No ready implementation milestone remains in this workstream.

A future architecture experiment must be registered separately rather than appending M006.

## 5. CI ownership rule

Hosted run `36866669727` failed at:

```text
cargo clippy --workspace --all-targets --locked -- -D warnings
```

All static authority guards and formatting before that step passed; workspace tests did not run.

C001 must reproduce the same command with the same default-feature semantics on current stable Rust.

Classify the diagnostic as:

- **owned** — introduced by commits in the retrieval-signal M003 implementation/closure path; fix in C001 with a focused regression;
- **unrelated-current-head** — caused by another concurrently landed subsystem or toolchain drift; record exact diagnostic and controlling owner/plan, and do not broaden C001;
- **transient/toolchain** — reproducibly attributable to stable-toolchain movement with no source defect; pin/guard only if repository policy requires it.

Do not infer ownership merely because the first red run occurred after M003.

## 6. Completion definition

C001 closes only when:

- CI failure is classified with exact diagnostic;
- any retrieval-signal-owned Clippy defect is fixed;
- roadmap and registry show terminal negative state consistently;
- no stale "only ready handoff", "M001R blocked", or "M002 blocked" prose remains;
- M004/M005 blockers are explicit;
- `scripts/verify.sh quick` passes;
- default workspace Clippy passes on the corrective head, or an unrelated registered blocker is explicitly cited;
- hosted CI evidence is recorded;
- additive closure record is committed.

The corrective itself unblocks no advisor milestone.
