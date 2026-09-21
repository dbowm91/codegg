# Tool-Selection Advisor — Post-Closure Model and Disclosure Corrective Addendum

Status: closing

Repository audit baseline: `c9087346620988a7c793fb2687732a62e481103a`

Predecessor work:

- `plans/subsystems/tool-selection-advisor-roadmap.md` — M001-M005 closed.
- `plans/closure/tool-selection-advisor/001-status.md` through `005-status.md` — historical closure evidence.
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` — controlling ADR; unchanged.
- `architecture/tool-advisor.md` — current data/runtime/training/telemetry description.
- `architecture/agent-tool-surface.md` — canonical tool-surface/disclosure authority.

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/003-planning-process.md`

Related ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` remains controlling.
- No new ADR is required for this corrective. The work restores the previously selected contextual-model and pre-turn-disclosure behavior without changing authority, locality, consent, or default-off policy. If implementation requires remote inference, default-on learned advice, a new execution authority, or a non-Rust required runtime, stop and register a separate ADR.

## 1. Purpose and corrective trigger

The original tool-selection advisor campaign closed after successfully establishing the surrounding machinery: versioned benchmark cases, deterministic keyword/BM25 baselines, an optional advisor abstraction, model artifact validation/fallback, Rust-only local training commands, consent-gated training-data plumbing, and experimental search reranking/promotion.

A post-closure audit found that those closures overstate two intended capability outcomes and leave one evidence/data-integrity gap:

1. **M003 did not produce the planned contextual small encoder.** The implemented `hashed-linear-v1` artifact is a useful 104-parameter lexical baseline. Training updates token weights derived from candidate name/description text and inference adds a fixed task/candidate token-overlap term. It does not learn a contextual function over task state and candidate descriptors, and it does not satisfy the roadmap statement that one reproducible small encoder recipe works locally.
2. **M005 promotion is reactive, not proactive.** `project_discovery()` is wired from `ToolSearchTool`; therefore learned promotion occurs only after the primary model already chose `tool_search`. The original M005 plan required promotion after authority filtering and before provider definitions are finalized so a tool-fragile primary model can see a useful deferred capability without first discovering `tool_search`.
3. **Qualification data is smoke-scale.** The current repository corpus contains 15 cases and the downstream qualification fixture contains four cases. The original M003 smoke split had one held-out test case, and M005 explicitly records no live external-primary-model trajectory evidence.
4. **The Cargo feature boundary is incomplete.** `tool-advisor` exists but currently gates no runtime code/dependency; this is harmless for the linear baseline but becomes material before introducing a real encoder/runtime.
5. **Training-event consent semantics are ambiguous.** `ToolAdvisorTrainingEvent` records `metadata_consent` and `content_consent`, while sink policy is independently configured. The current remote sink relies on configured remote/content policy rather than enforcing an authoritative event-level consent snapshot. The ambiguity should be removed before real trajectory collection grows.

This addendum preserves all predecessor closure records as historical evidence. It does not rewrite M001-M005. It owns the corrective work required to reach the original intended capability while retaining the successfully landed infrastructure.

## 2. Work classification

### Invariants

- CodeGG remains fully functional with no advisor feature, model, artifact, training feature, dataset capture, or remote telemetry configured.
- Advisor use remains opt-in and default-off.
- Training remains opt-in and local. No training command automatically calls a remote teacher, downloads a model, or submits data.
- Runtime inference remains in-process and pure Rust on the supported default path.
- The learned model may rank or promote only tools already present in the final policy-allowed/discoverable universe.
- The advisor never receives broker, permission, scheduler, sandbox, or execution authority.
- `hashed-linear-v1` remains a supported zero-cost baseline until explicitly superseded; it must not be relabeled as the contextual encoder.
- A contextual model must learn task/candidate interaction rather than candidate-only weights plus hand-written lexical overlap.
- Proactive promotion must occur before the provider-facing tool palette is finalized and must not require the main model to call `tool_search`.
- Telemetry remains separate from security/audit storage, network-silent by default, and content upload remains a separate explicit consent.
- Historical predecessor closures remain immutable.

### Capabilities

- A substantially larger, leakage-resistant benchmark/training corpus with held-out tool-family/generalization splits.
- A genuinely contextual approximately 5M-25M parameter local decision model trained entirely through Rust tooling.
- Optional pre-turn proactive promotion of a bounded number of policy-allowed deferred tools.
- Live/real-primary-model A/B qualification against smaller/tool-fragile models before any stronger product claim.

### Infrastructure

- Truthful Cargo feature isolation for neural runtime and training dependencies.
- Versioned contextual model artifact compatible with the existing `ToolAdvisor` abstraction.
- An authoritative consent snapshot/data-builder boundary rather than caller-authored consent booleans.
- Pre-provider-definition advisor projection in request preparation.

### Polish

- Resource/footprint reports on representative CPU targets.
- Clear operator diagnostics distinguishing linear baseline, contextual encoder, reactive rerank, and proactive promotion.
- Closure evidence that does not infer effectiveness from smoke fixtures.

## 3. Non-goals

- Replacing `ToolCatalog`, `tool_search`, `ResolvedToolSurface`, `ToolBroker`, or permission policy.
- Making the advisor default-on.
- Remote inference.
- Remote training jobs.
- Automatically collecting or uploading user repository content.
- Generating tool arguments with the advisor.
- General coding-model training.
- Model routing, planning, memory selection, compaction, or subagent-routing decisions; those remain separate future work.
- Rewriting the original M001-M005 closure records.
- Choosing model size by benchmark score alone without accounting for CPU latency/RSS/artifact size.

## 4. Current implementation evidence

At `c9087346620988a7c793fb2687732a62e481103a`:

- `src/tool_advisor/mod.rs` defines the reusable `ToolAdvisor` abstraction, `NoopAdvisor`, `LinearAdvisor`, artifact validation, candidate projection, and experimental discovery projection.
- `LinearAdvisor::score_inner` tokenizes context and candidate strings but learned weights are keyed only by candidate token terms; task influence is a fixed lexical-overlap term.
- `src/tool_advisor/training.rs` trains `hashed-linear-v1` candidate token weights and an abstention bias. The recorded smoke artifact had 104 parameters.
- `Cargo.toml` declares `tool-advisor = []` and `tool-advisor-training = ["tool-advisor"]`; the training module is feature-gated, but no production neural runtime is currently behind `tool-advisor`.
- `src/tool/tool_search.rs` is the sole production consumer of `project_discovery()`; therefore `promote` augments search results only after a search invocation.
- `src/agent/request_preparation.rs` owns the correct pre-dispatch seam: `project_initial_tool_palette`/`apply_tool_exposure_filter` decide which already-allowed definitions are immediately advertised versus deferred.
- `ResolvedToolSurface` remains the immutable authority projection and must stay upstream of any learned promotion.
- `src/tool_advisor/training_data.rs` provides default-off local spool/remote-ready plumbing with explicit endpoint configuration, but event consent flags and sink policy are two separate representations.
- The original M005 closure explicitly says no hosted/live smaller-primary-model trajectory improvement is claimed.

## 5. Corrective target architecture

### Data and training

```text
reviewed CodeGG cases
+ locally generated/imported cases
+ explicitly captured local trajectory events
                 |
                 v
versioned dataset builder
- provenance
- hard-negative families
- semantic-group splits
- tool-family holdouts
- consent snapshot normalization
                 |
                 v
        hashed-linear-v1 baseline
                 +
      contextual encoder-v1
      (approximately 5M-25M)
                 |
                 v
held-out calibration/evaluation
                 |
                 v
versioned local model artifact
```

### Runtime/disclosure

```text
registered/native/MCP/plugin tools
              |
existing deny/disable/plan/parent-ceiling filtering
              |
              v
      ResolvedToolSurface
              |
        +-----+------+
        |            |
        |       ToolAdvisor (optional)
        |       compact turn context
        |       allowed deferred candidates
        |            |
        |        calibrated scores
        |            |
        +-----+------+
              v
initial provider palette projection
(core/contextual + high-confidence promoted)
              |
              v
          primary LLM
              |
       tool_search still available
              |
              v
      broker/permission/sandbox
```

The learned advisor changes visibility only. It does not change the resolved authority universe.

## 6. Dependency graph

```text
M001 dataset + consent integrity
        |
        v
M002 contextual encoder/runtime/training --------+
                                                 |
M003 pre-turn proactive disclosure --------------+--> M004 live small-model qualification
   (independent of M001/M002; may use
    hashed-linear-v1 for wiring tests)
```

- M001 -> M002: hard. The encoder must train/evaluate against a corrected, leakage-resistant dataset contract.
- M003 is independently ready because the existing `ToolAdvisor`/linear implementation is sufficient to prove the correct pre-turn disclosure seam and authority invariants.
- M001 + M002 + M003 -> M004: hard.
- M004 may close with a negative effectiveness result, but it must not claim a useful contextual advisor unless the declared A/B gates are met.

## 7. Milestones

### M001 — Dataset, split, and consent integrity corrective

Class: data/invariant corrective.

Plan:

- `plans/implementation/tool-selection-advisor-post-closure-corrective/001-dataset-split-and-consent-integrity.md`

Objective: replace smoke-scale evaluation as the qualification basis with a larger provenance-aware corpus, tool-family/generalization holdouts, and one authoritative consent-snapshot boundary for training events.

Status: closing.

Exit conditions include a materially expanded reviewed corpus, leakage-resistant splits, explicit unknown-tool/tool-family holdouts, locally importable synthetic/teacher data without automatic remote calls, and remote sink tests proving an event cannot be transmitted under stale/absent effective consent.

### M002 — Contextual encoder runtime and local Rust training

Class: capability corrective.

Plan:

- `plans/implementation/tool-selection-advisor-post-closure-corrective/002-contextual-encoder-runtime-and-training.md`

Objective: implement and train the intended contextual decision model in the approximately 5M-25M range with a pure-Rust train/infer path, truthful feature isolation, and direct comparison against `hashed-linear-v1`.

Status: blocked on M001 closure.

Exit conditions include learned task/candidate interaction, contextual counterfactual tests, unknown-tool generalization, local Rust training, versioned artifacts, default-build dependency isolation, and resource/quality comparisons across at least two small capacity points.

### M003 — Pre-turn proactive tool disclosure

Class: capability/authority corrective.

Plan:

- `plans/implementation/tool-selection-advisor-post-closure-corrective/003-pre-turn-proactive-tool-disclosure.md`

Objective: move the useful `promote` behavior to the request-preparation palette seam so a primary model can see a relevant deferred tool without first invoking `tool_search`.

Status: ready.

Exit conditions include promotion before provider definitions are finalized, strict subset-of-allowed-surface evidence, bounded token/schema budget, failure/off equivalence, and a regression proving a promoted deferred tool is initially visible even when no `tool_search` call occurs.

### M004 — Live small-model trajectory qualification and corrective closure

Class: evidence/closure corrective.

Plan:

- `plans/implementation/tool-selection-advisor-post-closure-corrective/004-small-model-trajectory-qualification.md`

Objective: run pre-registered end-to-end A/B trajectories with representative smaller/tool-fragile primary models and determine whether contextual rerank/proactive promotion materially improve tool use.

Status: blocked on M001 + M002 + M003 closure.

Exit conditions include real primary-model trajectories, held-out task fixtures, cost/latency/tool-use measurements, resource evidence on representative local targets, and truthful mode disposition. Default remains off regardless of outcome.

## 8. Cross-cutting requirements

### Storage and migration

Dataset/event/model schemas remain explicitly versioned. Any training-event schema correction must retain an explicit reader/migration or fail with an actionable unsupported-version error. No security/audit database migration.

### Protocol and compatibility

No provider wire protocol changes. Provider-facing tool definitions continue to use existing `ToolDefinition` semantics; proactive promotion changes only the `defer_loading`/initial-visibility projection for already-allowed definitions.

### Security and authorization

Authority filtering happens before advisor candidate construction. Revalidate every promoted name against the same resolved surface before altering visibility. Model output is untrusted advisory data.

### Locality and packaging

Normal CodeGG must build/run without neural advisor dependencies or model weights. `tool-advisor` must become a real optional neural-runtime feature; `tool-advisor-training` must remain a stricter opt-in superset. No automatic model download.

### Telemetry and privacy

Local capture and remote contribution remain separate choices. Consent authority is host/config derived, not caller-authored. Remote transport remains no-endpoint/no-network by default. Content contribution requires a separate explicit content grant and redaction remains defense in depth.

### Performance

Record model parameter count, artifact/tokenizer size, release-binary delta, cold load, peak RSS, p50/p95 score latency, candidate-count scaling, and prompt/schema token delta. ARM64/SBC viability is part of model-size selection rather than an afterthought.

### Observability

Predictions must identify architecture/model version, context/candidate schema, surface fingerprint, scores, abstention, promotion decision, latency, and fallback reason without recording chain-of-thought.

## 9. Verification strategy

The corrective must prove both **mechanism** and **effectiveness**.

Mechanism evidence:

- no-feature/no-model/off equivalence;
- pure-Rust train and infer;
- policy-negative promotion tests;
- contextual counterfactual scoring;
- consent/no-network tests;
- pre-turn definition visibility tests.

Effectiveness evidence:

- expanded offline held-out corpus;
- unknown-tool/tool-family holdouts;
- `hashed-linear-v1` vs contextual encoder;
- BM25 vs learned rerank;
- off vs reactive rerank vs proactive promotion;
- real smaller-primary-model trajectories.

## 10. Risks and decision points

- A 5M encoder may be insufficient. Increase capacity only within the declared small-model comparison rather than jumping to a general LLM.
- A 20-25M model may be accurate but too costly on SBCs. Footprint/latency are co-equal acceptance inputs.
- Pretrained initialization may provide the strongest semantics, but model/tokenizer assets must be explicit local inputs with redistribution/license provenance; no hidden download.
- Large synthetic corpora can create leakage or teacher-style artifacts. Split by semantic group/tool family before generation where possible and record provenance.
- Proactive promotion can add prompt cost or distract stronger primary models. Qualification must measure false promotion and prompt-token delta, not only Recall@K.
- If a pure-Rust framework cannot satisfy supported-target/build constraints, stop and record the failure rather than introducing a required native/Python runtime.

## 11. Completion definition

This corrective closes when CodeGG has:

1. a credible evaluation/training corpus and unambiguous consent semantics;
2. a genuinely contextual small Rust-trained decision model with measured capacity/resource tradeoffs;
3. pre-turn proactive disclosure over the already-authorized tool surface;
4. real small-primary-model A/B evidence; and
5. unchanged normal operation with all advisor/model/training/telemetry features absent or disabled.

A negative A/B result is acceptable closure evidence. In that case the contextual model may remain an opt-in research/observe feature, and proactive promotion must not be represented as qualified for general use.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | ready | `plans/implementation/tool-selection-advisor-post-closure-corrective/001-dataset-split-and-consent-integrity.md` | — | none |
| M002 | blocked | `plans/implementation/tool-selection-advisor-post-closure-corrective/002-contextual-encoder-runtime-and-training.md` | — | M001 closure |
| M003 | ready | `plans/implementation/tool-selection-advisor-post-closure-corrective/003-pre-turn-proactive-tool-disclosure.md` | — | none; consumes closed predecessor runtime/surface |
| M004 | blocked | `plans/implementation/tool-selection-advisor-post-closure-corrective/004-small-model-trajectory-qualification.md` | — | M001 + M002 + M003 closure |
