# Tool-Selection Advisor — Evidence Integrity and Training Corrective Addendum

Status: active

Repository audit baseline: `71460c0cb1421f33a62d57123ac562c8a7c4bf1c`

Predecessor work:

- `plans/subsystems/tool-selection-advisor-roadmap.md` — original M001-M005 closed.
- `plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md` — corrective M001-M003 closed; M004 live qualification not yet executed.
- `plans/closure/tool-selection-advisor-post-closure-corrective/001-status.md` — corpus/consent closure now retained as historical evidence.
- `plans/closure/tool-selection-advisor-post-closure-corrective/002-status.md` — contextual-model closure now retained as historical evidence.
- `plans/closure/tool-selection-advisor-post-closure-corrective/003-status.md` — proactive-disclosure closure; authority placement remains valid.
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` — controlling ADR; unchanged.

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/003-planning-process.md`

No new ADR is required. This corrective repairs evidence integrity, training correctness, calibration, and candidate-shortlisting behavior without changing locality, authority, consent, or default-off product policy. If implementation requires remote inference/training, a default-on advisor, or a new execution authority, stop and register a separate ADR.

## 1. Corrective trigger

A post-closure source audit at `71460c0` found four defects that invalidate the current offline qualification basis even though the surrounding architecture is substantially improved.

1. **Cross-split content leakage.** The 256-case repository corpus has 128 declared semantic groups but only 40 unique context strings and 30 unique candidate descriptions. Independent audit found 42 exact input+label patterns crossing train/dev/test and 200/256 cases participating in those repeated cross-split patterns. `split_for()` hashes the declared semantic group, but the generator can assign different semantic-group identifiers to semantically and byte-for-byte equivalent payloads.
2. **Training-gradient correctness.** `contextual::train()` computes `error = probability - target`, passes `-learning_rate * error` into `update_pair()`, and `update_pair()` subtracts that scale times the feature gradient. The double negation makes the current update direction inconsistent with gradient descent. The mean-pooling token-count factor is also absent from per-token derivatives.
3. **Calibration/evaluation contract is incomplete.** The contextual runtime derives abstention from `sigmoid(-top_score)`; the training config exposes `calibration_temperature` but the contextual artifact/runtime does not consume calibrated temperature/bias. The generic contextual eval path can also report the entire supplied dataset as "test" rather than forcing a frozen qualification partition.
4. **Candidate cap precedes deferred eligibility filtering.** `candidates_from_surface(surface, max_candidates)` takes the first N resolved tools before M003 filters to deferred definitions. A relevant allowed deferred tool outside the first N can therefore be invisible to the advisor despite correct authority placement.

These defects were not detected by the prior closure because corpus lint validated counts/group identifiers rather than content-derived leakage, contextual tests proved only that context can affect a score rather than numerical gradient correctness/calibration, and proactive-disclosure tests used a tiny surface where the relevant deferred tool was already inside the first candidate window.

Historical closure records MUST NOT be rewritten. This addendum controls the corrective evidence required before the existing live M004 plan may execute.

## 2. Preserved accomplishments

The following predecessor work remains valid and should not be reimplemented:

- host-owned training-data consent snapshots and current-policy revalidation;
- no network/model download by default;
- `ToolAdvisor` / `NoopAdvisor` abstraction and safe fallback;
- `hashed-linear-v1` compatibility baseline;
- optional pure-Rust contextual artifact/runtime path;
- `ResolvedToolSurface` as authority owner;
- pre-turn promotion after resolved authority and before provider definitions;
- bounded promotion count/schema budget;
- permission/broker/sandbox independence;
- default-off advisor/training/telemetry behavior.

## 3. Invariants

- Normal CodeGG remains correct with advisor features absent, disabled, missing, or corrupt.
- Advisor/training/telemetry remain explicit opt-ins.
- Training and inference remain local/pure Rust on the supported path.
- No tool omitted by `ResolvedToolSurface` may enter advisor candidates or promotion output.
- Dataset split membership is derived from leakage-resistant content/template identity, not merely caller-supplied arbitrary IDs.
- Final test and family-holdout examples are never used for optimizer updates, calibration, threshold selection, architecture selection, or early stopping.
- A reported parameter count is storage/model size information, not evidence of effective learned capacity.
- Training math is backed by numerical gradient tests and a monotonic tiny-overfit fixture.
- Calibration parameters used at runtime are learned/frozen from development data and serialized in the artifact.
- Candidate limits apply after authority and deferred-eligibility filtering; large deferred sets use deterministic semantic/lexical preselection rather than resolved-surface position.
- Existing M004 live provider qualification remains blocked until the offline corrective C004 closes with a positive disposition.

## 4. Non-goals

- Rewriting M001-M003 closure records.
- Broad replacement of `ToolCatalog`, `ResolvedToolSurface`, `ToolBroker`, or request preparation.
- General-purpose language-model training.
- Introducing Python/PyTorch, remote trainers, or mandatory native ML runtimes.
- Automatically collecting real user repository content.
- Increasing contextual model size merely to improve headline parameter count.
- Running paid/live primary-model trajectories as part of C001-C003.

## 5. Current-state evidence

### Corpus

At the audit baseline:

- 256 cases;
- 128 declared semantic groups;
- 40 unique context strings;
- 30 unique candidate descriptions;
- 42 exact input+label patterns cross train/dev/test;
- 200 cases participate in those repeated cross-split patterns;
- current family "holdout" records a family fingerprint but training does not implement a general partition contract that guarantees held-out family exclusion from optimizer/calibration use.

### Contextual scorer

`contextual-embedding-v1` is a hashed-token mean-embedding interaction scorer:

```text
mean(context token embeddings)
        dot
mean(candidate token embeddings)
        / embedding_dim
        + bias
```

The small and medium artifacts allocate 65,536 rows x 80/240 dimensions. The current corpus contains only about 148 unique normalized tokens, so physical parameter count substantially exceeds the number of embedding rows that can receive data-derived updates. This does not make the architecture invalid, but qualification must report active rows/collisions and compare useful quality per byte/latency rather than treating 5.2M/15.7M as proof of learned complexity.

### Disclosure candidate construction

M003 correctly moved promotion to request preparation, but `project_preturn_promotions()` calls `candidates_from_surface()` with the cap before intersecting with deferred definitions. Surface ordering can therefore determine advisor visibility.

## 6. Dependency graph

```text
C001 corpus/split integrity
        |
        v
C002 training math + calibration
        |
        +--------------------+
                             |
C003 deferred-first shortlist -----> C004 clean offline requalification
                                      |
                                      v
                     existing M004 live primary-model qualification
```

- C001 is ready.
- C003 is independently ready and may execute in parallel with C001.
- C002 is blocked on C001 because final numerical/quality evidence must be generated against the corrected frozen partitions, even though implementation may reuse existing runtime interfaces.
- C004 is blocked on C001+C002+C003.
- Existing post-closure M004 is blocked on a **positive C004 qualification disposition plus its original live-provider prerequisites**.

## 7. Corrective milestones

### C001 — Content-derived corpus and split integrity

Plan:

- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/001-content-derived-corpus-split-integrity.md`

Status: ready.

Repair corpus generation and validation so exact/normalized-equivalent inputs, template variants, and counterfactual families cannot leak across partitions. Introduce true optimizer-excluded tool-family holdouts and regenerate/freeze trustworthy dataset fingerprints.

### C002 — Contextual training math, calibration, and capacity truthfulness

Plan:

- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/002-contextual-training-math-calibration.md`

Status: blocked on C001.

Correct gradient descent, mean-pooling derivatives, train/dev/test discipline, and runtime abstention calibration. Add numerical-gradient/overfit guards and report active embedding rows/effective trained footprint.

### C003 — Deferred-first candidate shortlisting

Plan:

- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/003-deferred-first-candidate-shortlisting.md`

Status: ready.

Filter to final eligible deferred tools before applying candidate limits. For catalogs larger than the neural budget, use deterministic lexical/BM25 preselection over the full eligible deferred universe before contextual scoring.

### C004 — Clean offline requalification and live-M004 gate

Plan:

- `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/004-clean-offline-requalification.md`

Status: blocked on C001+C002+C003.

Rerun baselines and contextual models on clean frozen partitions, true family/unknown-tool holdouts, counterfactual slices, and candidate-recall tests. Only a positive pre-registered disposition may unblock paid/live M004 trajectories.

## 8. Cross-cutting verification

Every milestone must preserve:

- `cargo check --locked` default build;
- advisor feature builds;
- training feature builds;
- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
- `scripts/verify.sh quick`;
- `git diff --check`.

C004 additionally requires hosted CI evidence for its closing commit when available. Local-only green tests are insufficient to erase a known data/model correctness finding.

## 9. Completion definition

This corrective closes when:

1. no exact or normalized-equivalent input family crosses train/dev/test;
2. true tool-family holdouts are excluded from training/calibration;
3. contextual gradient/calibration behavior is numerically verified;
4. runtime artifact carries the calibration actually used during scoring/abstention;
5. candidate shortlist coverage is independent of resolved-surface position;
6. clean offline metrics are rerun against immutable fingerprints;
7. current model architecture is either positively qualified on those clean slices or explicitly demoted to a research baseline; and
8. the registry truthfully gates the existing live M004 plan on that disposition.

## 10. Milestone status

| Milestone | Status | Plan | Blocker |
|---|---|---|---|
| C001 | ready | `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/001-content-derived-corpus-split-integrity.md` | none |
| C002 | blocked | `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/002-contextual-training-math-calibration.md` | C001 |
| C003 | ready | `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/003-deferred-first-candidate-shortlisting.md` | none |
| C004 | blocked | `plans/implementation/tool-selection-advisor-evidence-integrity-corrective/004-clean-offline-requalification.md` | C001+C002+C003 |
