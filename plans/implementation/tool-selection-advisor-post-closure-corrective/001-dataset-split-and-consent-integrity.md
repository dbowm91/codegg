# Tool-Selection Advisor Post-Closure Corrective M001 — Dataset, Split, and Consent Integrity

Status: ready for handoff

Repository baseline: `c9087346620988a7c793fb2687732a62e481103a`

Source corrective:

- `plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md#m001--dataset-split-and-consent-integrity-corrective`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Predecessor evidence:

- `plans/closure/tool-selection-advisor/001-status.md`
- `plans/closure/tool-selection-advisor/004-status.md`

Primary class: data/invariant corrective.

## 1. Objective

Make the advisor dataset and training-data consent semantics strong enough to support real contextual-model training and qualification.

M001 must expand the current smoke-scale corpus, make train/dev/test and unknown-tool evaluation resistant to semantic leakage, preserve provenance for every case, and remove ambiguity between caller-authored event consent flags and host/config-derived consent.

No neural runtime or proactive disclosure is introduced here.

## 2. Why this milestone is ready

The predecessor M001 and M004 infrastructure is already closed and provides versioned case/event structures, deterministic split helpers, local spool/export/purge, explicit remote endpoint configuration, and no-network defaults.

The defects are in evidence scale and consent authority, not missing architectural dependencies.

## 3. Current evidence and defect

At the baseline:

- `assets/tool-advisor/corpus.jsonl` contains 15 reviewed cases.
- The predecessor training closure recorded 10 train, 4 dev, and 1 held-out test case.
- `assets/tool-advisor/downstream-suite.jsonl` contains four qualification cases.
- Unknown-tool examples exist but are too small to establish generalization.
- `split_for(group_id)` provides deterministic grouping, but the corpus does not yet exercise broad semantic/tool-family holdouts.
- `ToolAdvisorTrainingEvent` includes `metadata_consent` and `content_consent`, while `TrainingDataPolicy` independently gates local/remote/content behavior.
- `RemoteSink` validates configured policy and sanitizes content, but event consent fields are not an independently authoritative transmission gate.

## 4. Invariants

- No automatic network calls, model downloads, remote teacher requests, or telemetry upload.
- Repository fixtures contain no private repository content.
- Local imported/generated data must declare provenance.
- Train/dev/test grouping is stable and reproducible.
- Near-duplicate/paraphrase variants cannot cross held-out semantic groups.
- Unknown-tool tests must not be solvable solely from current built-in tool names.
- Remote transmission requires one host-derived effective consent state; callers cannot self-authorize by setting booleans in an event.
- Metadata-only consent never implies content consent.

## 5. Scope

### In scope

- Corpus expansion and coverage matrix.
- Semantic group/tool-family split support.
- Leave-one-tool-family-out/unknown-tool evaluation.
- Hard-negative families and no-tool/multi-tool balance.
- Locally generated/imported case ingestion with provenance and validation.
- Optional soft teacher probabilities in the case schema when supplied from a local/offline generation workflow.
- Event consent-snapshot correction and schema/version handling.
- Local spool/remote sink tests proving effective consent.
- Dataset lint/report commands.

### Out of scope

- Neural model code.
- Automatic external teacher generation.
- Remote data collector operation.
- Pre-turn disclosure.
- Live provider A/B qualification.

## 6. Dataset requirements

Create a qualification corpus large enough that single cases cannot dominate headline metrics. Unless an implementation review documents a stronger coverage-based threshold, use the following minimum floor:

- at least **256 cases**;
- at least **128 semantic groups**;
- at least **64 hard-negative cases**;
- at least **32 explicit no-tool/abstention cases**;
- at least **32 multi-tool relevance cases**;
- at least **32 unknown/synthetic-tool cases**;
- coverage across native filesystem/search, Git, LSP, verification/test, research/network, context/goal/work-plan/work-order, plugins/MCP, shell fallback, structured data, and no-tool language-only tasks.

Counts are floors, not success metrics. Cases must be semantically reviewed or generated from templates whose group/provenance prevents leakage.

### Split rules

Add explicit split metadata capable of representing:

- semantic group;
- task family;
- tool family;
- provenance/source;
- generated variant family.

The final held-out test split must be frozen before M002 architecture tuning.

Add at least one **tool-family holdout** where a tool family is absent from training but textual descriptors are available at inference. Add a renamed/synthetic-name holdout to prevent canonical-name memorization.

### Counterfactual pairs

Add paired cases where:

- the candidate set is identical;
- the task/context changes;
- the correct tool changes.

These pairs become a hard gate for M002: a contextual model should change rankings while a candidate-only memorizer should fail.

## 7. Consent correction

Do not leave consent authority as arbitrary booleans supplied by event producers.

Preferred shape:

1. create a host-owned `TrainingConsentSnapshot` or equivalent from `TrainingDataPolicy`;
2. event construction receives that snapshot from the host rather than free-form booleans;
3. the snapshot records local-capture, metadata-remote, and content-remote grants separately;
4. the local and remote sinks validate the snapshot against current sink policy before persistence/transmission;
5. remote metadata transmission fails when effective metadata consent is absent;
6. remote content requires metadata remote consent + content remote consent + configured `remote_include_content`;
7. turning remote consent off prevents subsequent transmission even for queued/previously captured events;
8. consent fields remain provenance/audit metadata, never the sole authority for a caller to enable transport.

If preserving the v1 event schema, define explicit compatibility semantics. If introducing v2, add a bounded reader/migration or reject unsupported records with actionable diagnostics.

## 8. Ordered work packages

### A — Coverage inventory and schema tests

Produce a machine-readable corpus coverage report and add tests for semantic groups/tool families/provenance.

### B — Corpus expansion

Add reviewed and locally generated/imported cases to reach the declared floors. Do not use real private user repository text.

### C — Leakage-resistant splits

Implement/freeze semantic-group and tool-family holdouts plus counterfactual-pair validation.

### D — Consent authority correction

Introduce the host-derived consent snapshot/factory and remove caller-authored consent as transport authority.

### E — Sink and lifecycle regression tests

Prove default no-file/no-network, local-only behavior, remote metadata-only behavior, content gating, consent revocation, inspect/export/purge, and schema compatibility.

### F — Baseline refresh

Record keyword/BM25/`hashed-linear-v1` results on the expanded frozen splits. These become the comparison baseline for M002/M004.

## 9. Failure/recovery semantics

Malformed imported cases are rejected without partial corpus installation. Dataset generation/import should write through a staging file/directory then atomically publish validated output where practical.

Consent/sink errors remain non-fatal to agent execution. Turning remote off immediately prevents future send attempts. Corrupt spool records remain quarantined rather than blocking CodeGG.

## 10. Required tests

- corpus count/coverage floors;
- duplicate/near-duplicate group guard;
- semantic-group split isolation;
- tool-family holdout isolation;
- counterfactual-pair validation;
- unknown-name holdout;
- provenance required for imports;
- default policy cannot construct an active remote sink;
- event producer cannot self-authorize remote transmission;
- remote metadata requires effective metadata grant;
- content requires independent effective content grant;
- revoked remote policy prevents send;
- v1/v2 compatibility behavior if schema changes;
- no-network/no-file default regression.

## 11. Verification commands

Use exact implemented command names. Expected minimum:

```bash
cargo test -p codegg --lib tool_advisor
cargo test -p codegg --lib tool_advisor::training_data
cargo run --locked --bin codegg -- tool-advisor bench --dataset assets/tool-advisor/corpus.jsonl --json
cargo run --locked --bin codegg -- tool-advisor data status
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Also record the final corpus coverage matrix and frozen split fingerprints in closure.

## 12. Documentation updates

- `architecture/tool-advisor.md`: dataset provenance/split contract and consent authority.
- Fixture contributor/import documentation.
- Explicit explanation that no automatic teacher/remote generation exists.
- Data-field/consent matrix.

## 13. Acceptance criteria

M001 closes only when:

1. the declared corpus/coverage floors are met with provenance;
2. semantic-group and tool-family holdouts are frozen and reproducible;
3. counterfactual pairs exist for contextual-model testing;
4. keyword/BM25/linear baselines are rerun on the expanded splits;
5. effective consent is host/config derived rather than caller self-asserted;
6. remote metadata/content gating and revocation are executable tests;
7. default CodeGG remains network/file silent for advisor training data.

## 14. Stop conditions

Stop if corpus expansion requires uploading user data, automatic external teacher calls, weakening consent, or mixing training records with security audit storage.

## 15. Closure evidence required

- corpus counts/coverage by family/tag/provenance;
- train/dev/test/tool-family holdout fingerprints;
- duplicate/leakage analysis;
- baseline metrics;
- consent-state matrix;
- no-network/revocation test evidence;
- schema compatibility disposition;
- exact verification output.
