# Tool-Selection Advisor Retrieval-Architecture — Closure Corrective Addendum

Status: closed (C001 closed; see `plans/closure/tool-selection-advisor-retrieval-architecture-closure-corrective/001-status.md`; implementation `220d3638`; hosted CI run `35859251981`)

Repository planning baseline: `5864e5497d23f65fdec0390ac65c4173d92af2dd`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`
- `plans/003-planning-process.md#7-corrective-passes`

Predecessor work:

- `plans/subsystems/tool-selection-advisor-retrieval-architecture-experiment-roadmap.md`
- `plans/closure/tool-selection-advisor-retrieval-architecture-experiment/003-status.md`
- implementation/closure head `5864e5497d23f65fdec0390ac65c4173d92af2dd`
- hosted CI run `35829020175` — failed at `Execution ownership guard`

## 1. Purpose

The retrieval-architecture experiment reached a valid negative technical conclusion, but its closure is not repository-clean.

Post-closure review found three bounded defects:

1. `src/tool_advisor/retrieval_architecture.rs` directly invokes `git rev-parse HEAD` through `std::process::Command::new` to populate sweep provenance. That call is not classified in `docs/execution-ownership.toml` and is not routed through a governed execution service. Hosted CI therefore fails at `scripts/check_execution_ownership.py`.
2. The retrieval-architecture roadmap still declares `Status: active` even though the registry and M003 closure say the workstream is closed.
3. The roadmap milestone table contains a stale duplicate `M002 | not started` row beneath the actual closed M002 row.

The original M003 recall verdict remains valid historical evidence. This corrective owns only closure hygiene and repository verification.

## 2. Why original verification missed the defect

The M003 closure ran focused retrieval tests, focused Clippy, formatting, and `git diff --check`, but explicitly did not run the repository-wide quick verification guard.

`scripts/verify.sh quick` includes:

```text
python3 scripts/check_execution_ownership.py
```

Hosted CI also runs this guard before formatting, Clippy, or workspace tests. Run `35829020175` therefore failed before those later jobs could execute.

The roadmap-state drift was a documentation reconciliation omission during closure, not an implementation failure.

## 3. Corrective scope

One milestone only:

- `plans/implementation/tool-selection-advisor-retrieval-architecture-closure-corrective/001-execution-ownership-and-planning-closure.md`

Status: closed (see `plans/closure/tool-selection-advisor-retrieval-architecture-closure-corrective/001-status.md`; implementation `220d3638`; hosted CI run `35859251981`).

The milestone removes the experiment-owned Git subprocess, repairs roadmap state, runs the canonical repository guards, and adds supplemental closure evidence.

## 4. Invariants

This corrective MUST NOT:

- change retrieval scoring, fusion, pooling, candidate universes, shortlist K, or recall gates;
- rerun or reinterpret the expensive R001 frontier as a new selection attempt;
- change the selected M003 span-packed ranker;
- create a v4 holdout or unblock order-invariance M005;
- alter promotion thresholds or runtime tool disclosure;
- mutate historical M001/M002/M003 closure records;
- add a runtime/daemon subprocess merely to retain experiment provenance.

ADR-0009 locality, authority monotonicity, default-off behavior, and historical corpus immutability remain unchanged.

## 5. Corrective disposition

A positive C001 closure means only:

- current `main` is repository-clean for this workstream;
- the retrieval-architecture roadmap and registry agree that the workstream is closed;
- the negative M003 recall verdict remains the controlling technical result.

It does **not** make any downstream advisor milestone ready.

## 6. Completion definition

The corrective closes only when:

- the direct Git subprocess is removed from the experiment module;
- sweep provenance is supplied explicitly by the operator/caller and remains fingerprint-bound;
- execution-ownership guard passes;
- `scripts/verify.sh quick` passes;
- focused retrieval-architecture tests pass;
- formatting and Clippy pass;
- hosted CI on the corrective implementation commit completes successfully;
- roadmap stale status/duplicate row are fixed;
- an additive corrective closure record is written.

Until then, the predecessor technical workstream remains historically closed but repository closure is incomplete.
