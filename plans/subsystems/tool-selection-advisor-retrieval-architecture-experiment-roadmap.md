# Tool-Selection Advisor Retrieval-Architecture Experiment Roadmap

Status: active

Repository planning baseline: `ccd548515f59f082c64a128c3b8ab3079ddf9f88`

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#cross-phase-execution-rules`

Related ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Predecessor evidence:

- `plans/closure/tool-selection-advisor-order-invariance-experiment/004-status.md` — negative: best recall 0.9444 vs gates 0.99/0.98/0.95 at K<=32, zero violations.
- `plans/closure/tool-selection-advisor-order-invariance-experiment/003-status.md` — positive span-packed selection (frozen ranker, reused unchanged here).
- `assets/tool-advisor/order-invariance-m003-selection.json` — M003 selection receipt.
- `plans/closure/tool-selection-advisor-qualification-evidence-corrective/002-status.md` — semantically valid v3 disposition D (immutable diagnostic only).

## 1. Purpose and ownership boundary

This workstream owns the retrieval-scoring question that M004 closed negatively: no K<=32 configuration of the four predeclared modes (BM25 / semantic / RRF / normalized-union) clears the large-catalog recall gates on dev, and the miss is systematic rather than a cutoff edge.

Observed frontier (dev, 62 cases / 72 relevant tools per universe, sweep `0e802d79…`):

- BM25 0.7222 flat across K=16/24/32 and all three universes (20 tools rank beyond K32).
- Semantic 0.8889-0.9028, nearly flat with K (7-8 tools rank beyond K32).
- Normalized-union 0.9444 ceiling from K16 (4 tools missed by both signals at every universe).
- RRF is the only K-sensitive mode (e.g. 0.7361 -> 0.9444 at universe 64) but still caps at 0.9444.

The workstream may change retrieval scoring and fusion and may measure larger shortlists only with bounded latency/schema/ranker-cost analysis. It owns no ranker training, no promotion-threshold logic, no v4 holdout, and no runtime discovery path. Promotion re-attachment and fresh qualification happen only after a positive operating-point selection, as separately registered plans.

## 2. Work classification

### Invariants

- ADR-0009 authority monotonicity: retrieval consumes only already-authorized deferred descriptors; encoder failure falls back to BM25; zero authority violations.
- Historical train/dev/test, v2, v3 corpora immutable; v3 never tunes retrieval, thresholds, K, or architecture.
- Deterministic retrieval: name tiebreaks, versioned protocol, fingerprinted inputs.

### Capabilities

- None completed in this workstream. A positive operating-point selection is infrastructure until a separate fresh-holdout qualification passes.

### Infrastructure

- M001 miss diagnostics plus preregistered selection contract.
- M002 new retrieval/fusion variants behind the existing authority boundary.
- M003 extended frontier measurement with bounded-K analysis and operating-point selection.

### Polish

- None planned. Latency/schema reporting is gating evidence, not polish.

## 3. Non-goals

- No ranker retraining or architecture change; the M003 span-packed artifact is frozen input.
- No promotion-threshold redesign; M004 promotion code is reused unchanged once retrieval clears.
- No v4 holdout creation in this workstream; a positive M003 registers a separate v4 qualification plan.
- No reinterpretation of the order-invariance M004 negative closure or the v3 disposition D.
- No silent shortlist increase: any K beyond 32 requires the preregistered latency/schema/ranker-cost analysis.
- No new remote, download, telemetry, or sidecar path; no production discovery-path change.

## 4. Current state

M004 implemented the preregistered frontier (`m004-preregistered-operating-point-v1`) with per-universe checkpoint/resume and fail-closed gate enforcement in `src/tool_advisor/operating_point.rs` over `src/tool_advisor/sequence_retrieval.rs` (BM25 via the catalog baseline, MiniLM mean-pooled cosine semantic, RRF with fixed RRF_K=60, equally weighted normalized-union). The sweep ran once to completion on dev; promotion, resource, and v3 stages were correctly never reached. Frontier receipts exist only as local run artifacts plus the closure-record tables; no operating point was frozen. The order-invariance M005 stays hard-blocked and is historical; this workstream does not unblock it.

## 5. Target architecture

On dev-only evidence, select at most one frozen retrieval operating point (mode, parameters, shortlist K, universe identity) that clears the M004 gates at K<=32, or — only with preregistered budgets satisfied — at a bounded larger K. The point reuses the frozen span-packed ranker and the M004 promotion separator downstream. Everything new lives in the experiment module; catalog BM25, the resolved surface, the broker, and the default discovery path are untouched.

Allowed variant space for M002 (M001 preregisters the exact grid):

- Fusion tuning: alpha-weighted normalized union over a preregistered alpha grid, RRF_K ablation over preregistered values, max-fusion.
- Query/descriptor alignment: pooling ablation (CLS vs mean) for query and descriptor sides, descriptor normalization variants (e.g. disclosure-field handling), advisor-side lexical signal that does not alter catalog BM25.
- Two-stage re-rank: union top-N shortlist (preregistered N) re-ranked by the frozen M003 span-packed ranker down to top-K, with ranker-cost accounting and permutation-invariance checks.
- Bounded shortlist extension: K=48/64 measured only with latency p95/max, schema, cold-load, and ranker-cost budgets; K<=32 remains the primary gate.

## 6. Dependency graph

```text
M001 miss diagnostics + preregistration
              |
              v (hard)
M002 retriever/fusion variants
              |
              v (hard)
M003 extended frontier + operating-point selection
              |
              v (hard, only on positive M003)
fresh v4 qualification (separately registered new plan, not order-invariance M005)
```

- M001 hard deps: order-invariance M001-M004 closed (M004 negative), M003 selection receipt stable. Interface dep: `sequence_retrieval` / `operating_point` contracts stable per ADR-0009.
- M002 hard dep: M001 preregistration closed.
- M003 hard dep: M002 variants landed; interface dep: M004 promotion separator reused unchanged.
- Soft dep: M004 checkpoint/resume pattern reused.
- Old order-invariance M005 is not a downstream of this workstream; it stays blocked as historical evidence.

## 7. Milestones

### M001 — Retrieval-miss diagnostics and preregistration

Class: infrastructure

Objective: attribute the four systematic misses (BM25 rank vs semantic rank vs fusion margin per missed tool) and freeze the selection contract: protocol name/version, variant grid, universe/K grid, gates, budgets, exact sweep command, checkpoint/resume and fail-closed rules.

Dependencies: hard on order-invariance M003 positive and M004 negative closures; interface on ADR-0009 and the retrieval module contracts.

Deliverable boundary: diagnostic attribution code with synthetic-fixture unit tests plus a preregistration constants block; no encoder sweep, no variant implementation, no gate changes.

User or operator value: none directly; unblocks M002.

Exit conditions: miss attribution recorded; every M002/M003 degree of freedom preregistered; M002 ready.

Deferred work: variant implementation (M002), extended sweep (M003).

### M002 — Retriever and fusion variants

Class: infrastructure

Objective: implement exactly the preregistered variant space behind the existing authority boundary (fallback, cache hygiene, determinism preserved).

Dependencies: hard on M001.

Deliverable boundary: new scoring/fusion code plus focused unit tests (fusion math, determinism, fallback, universe/K identity); no selection, no threshold logic.

User or operator value: none directly; unblocks M003.

Exit conditions: all preregistered variants constructible and unit-tested; M003 ready.

Deferred work: frontier measurement and selection (M003).

### M003 — Extended frontier and operating-point selection

Class: infrastructure

Objective: run the preregistered sweep once on dev, select the smallest/cheapest point clearing 64>=0.99, 128>=0.98, 256>=0.95 with zero violations (K<=32 primary; bounded larger K only with budgets met), then run the M004 promotion separator unchanged at the frozen point.

Dependencies: hard on M002; interface on the M004 promotion separator.

Deliverable boundary: one sweep receipt set, at most one frozen operating point, promotion re-attachment evidence; no v3 tuning (diagnostic only after freeze, no parameter changes).

User or operator value: none directly; a positive close registers a separate fresh v4 qualification plan.

Exit conditions: positive close (point frozen, v4 plan registered) or plan-literal negative close (no point, nothing downstream unblocked).

Deferred work: v4 holdout and qualification (separate plan).

## 8. Cross-cutting requirements

### Storage and migration

Additive experiment code and versioned protocol constants only. No storage migration, no artifact format change, no historical asset mutation. Run receipts live under `target/tool-advisor/retrieval-architecture/` with fingerprint-bound checkpoint/resume; closure tables carry the evidence.

### Protocol and compatibility

New protocol name (M001 preregisters, e.g. `r001-preregistered-retrieval-architecture-v1`) with its own schema version; M004 protocol constants untouched. Frontier points keep explicit `candidate_universe_size` + `shortlist_k` identity; missing/duplicated points fail closed.

### Security and authorization

ADR-0009 holds: deferred-only input, no descriptor-cache contamination with user context, BM25 fallback on encoder failure, denied/hidden tools never enter any universe. Gate: zero authority violations.

### Concurrency, cancellation, and recovery

Sweep supports per-universe checkpoint/resume bound to the sweep fingerprint (M004 pattern); duplicate launch or SIGTERM resumes instead of recomputing. Diagnostic work in M001 needs no encoder run.

### Observability and audit

Each frontier point records mode, parameters, universe, shortlist K, relevant/recovered counts, recall, latency distribution, and violations. Miss attribution in M001 records per-tool BM25/semantic ranks and fusion margins.

### Performance and resource use

M001 preregisters budgets: retrieval p50/p95/max per K, ranker re-rank cost per case (two-stage only), process-cold load p95, encoder weights, total sweep wall-clock with resume. Prior resource envelope (cold load p95 <=10 s, encoder <=128 MiB) is preserved unless a separate plan changes it. M004 precedent: ~65 min sweep with first-embedding-load outliers; the new plan budgets or caches encoder load explicitly.

### Documentation and operations

No user-facing docs change (experiment-only). Planning docs updated per lifecycle: roadmap status, registry rows, closure records.

## 9. Verification strategy

- Unit: fusion math (alpha weighting, RRF_K, normalization edge cases), determinism under permutation, fallback on encoder failure, universe/K identity and fail-closed selection, miss-attribution correctness on synthetic fixtures.
- Integration: extended frontier sweep runs once per preregistration on dev (ignored long test, serial, checkpointed); promotion separator regression tests reused unchanged.
- Negative: missing-point, duplicate-point, authority-violation, and budget-exceeded cases all fail closed with explicit errors.
- No workspace-wide sweep is claimed unless run; closures label local vs CI evidence truthfully.

## 10. Risks and decision points

- The four misses may share one cause (e.g. descriptor/query vocabulary gap) or four independent causes; M001 attribution decides whether the variant space needs widening via roadmap amendment rather than silent scope growth.
- Larger K is unlikely to clear the 64>=0.99 bar given flat union curves; the primary bet is scoring/fusion, with extended K as a bounded secondary.
- Two-stage re-rank couples retrieval cost to the packed ranker; if ranker latency breaks the turn-side envelope, M003 must close negatively on resources rather than silently accept.
- No ADR is required: no identity, ownership, protocol, or authorization boundary changes.

## 11. Completion definition

The workstream closes when M003 closes. Positive M003 freezes one operating point and registers a separate fresh v4 qualification plan (new number in this workstream, not order-invariance M005). Negative M003 leaves live primary-model M004 blocked with recorded evidence. Either way, historical order-invariance closures and the v3 disposition D are untouched.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/tool-selection-advisor-retrieval-architecture-experiment/001-retrieval-miss-diagnostics-and-preregistration.md` | `plans/closure/tool-selection-advisor-retrieval-architecture-experiment/001-status.md` | none |
| M002 | closed | `plans/implementation/tool-selection-advisor-retrieval-architecture-experiment/002-retriever-and-fusion-variants.md` | `plans/closure/tool-selection-advisor-retrieval-architecture-experiment/002-status.md` | none |
| M002 | not started | — | — | M001 |
| M003 | active | `plans/implementation/tool-selection-advisor-retrieval-architecture-experiment/003-extended-frontier-and-operating-point-selection.md` | — | M002 |
