# Tool-Selection Advisor Milestone 001 — Evaluation Corpus and Baselines

Status: implemented

Repository baseline: `ed960f2ae7043acd816970f7d06e74dc69b09ae8`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-roadmap.md#m001--evaluation-corpus-schemas-and-deterministic-baselines`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure

## 1. Objective

Create the reproducible, pure-Rust evaluation/data foundation required to judge a future tiny tool-selection model without adding an ML runtime or changing agent behavior.

M001 must define versioned task/candidate/label schemas, a curated benchmark corpus, deterministic split/holdout logic, and baseline evaluation for the current keyword/BM25 discovery machinery. The benchmark must explicitly include hard negatives, abstention/no-specialized-tool cases, multi-tool relevance, and tools withheld from training-style splits to measure descriptor-based generalization.

## 2. Why this milestone is ready

The closed coding-agent tool-surface work provides the stable inputs M001 needs:

- live `ToolCatalog` metadata;
- keyword/BM25 search;
- policy-filtered `ResolvedToolSurface`;
- compact `tool_search` descriptors;
- stable registered/advertised/discoverable/callable semantics.

No learned runtime, storage migration, telemetry transport, or training framework is needed.

## 3. Current implementation evidence

At the baseline:

- `src/tool/catalog.rs` owns keyword/BM25 ranking of name + description.
- `src/tool/tool_search.rs` caps broad search results and returns compact selection descriptors before exact schema expansion.
- `architecture/agent-tool-surface.md` documents the resolved per-turn surface and discovery universe.
- There is no canonical tool-selection case schema or benchmark report.
- Existing tool-surface tests prove authority/disclosure correctness but do not answer ranking-quality questions such as Recall@K, MRR, nDCG, no-tool calibration, or unknown-tool generalization.

## 4. Invariants that must not regress

- No new production model dependency.
- No new network access.
- No tool-surface behavior change.
- Benchmark candidate sets use the same semantic descriptors as real discovery wherever possible.
- Labels do not imply authority: a benchmark may say a tool is semantically useful even if a separate policy fixture removes it, but production candidate construction always remains policy-gated.
- Tool IDs/names are not the only predictive feature in unknown-tool tests.

## 5. Scope

### In scope

- Internal versioned Rust structs for benchmark cases, candidate descriptors, labels, predictions, and metrics.
- A stable serialization format suitable for repository fixtures and future local training events, preferably JSON/JSONL with explicit schema version.
- Curated seed corpus covering ordinary coding, research, Git, LSP, verification, context/goal/work-plan/work-order, security, plugin/MCP, and no-tool cases.
- Hard-negative groups (for example read vs grep vs code/repo search; LSP vs text search; verify vs shell; Git inspection vs raw shell; built-in vs similarly described MCP tool).
- Multi-label relevance and optional preferred-tool ordering.
- Deterministic train/dev/test grouping and leave-one-tool-out/unknown-tool holdouts.
- Keyword/BM25 baseline evaluator and report.
- Dataset validation/static guards.
- Developer-facing command/test entry point that does not affect normal runtime.

### Explicitly out of scope

- Learned inference.
- Model weights/tokenizers.
- Training.
- Session telemetry/capture.
- Remote datasets/services.
- Main-agent prompt or disclosure changes.

## 6. Required production changes

### Core/domain

Define a bounded advisor data contract, likely in a small internal module/crate that can later be consumed by runtime and training without depending on TUI/provider code.

A case should represent at least:

- schema version and case ID;
- compact task/context text;
- candidate descriptors with canonical name, purpose/description, category/disclosure semantics, and optional synthetic/unknown identity;
- one or more relevant candidates with graded or binary relevance;
- explicit `none`/abstention label;
- tags for domain/difficulty/hard-negative family;
- split/group identity preventing near-duplicate leakage;
- source/provenance classification.

Do not copy full tool JSON schemas into every case unless a specific benchmark needs them.

### Evaluation

Implement deterministic metrics:

- Recall@1/@3/@5;
- MRR;
- nDCG or equivalent graded ranking metric;
- no-tool precision/recall/F1 for systems that expose abstention;
- candidate coverage;
- per-domain and hard-negative-family breakdown.

ECE/Brier/log-loss belong once probabilistic learned predictions exist, but the schema should be able to accept probabilities without a migration.

### Fixtures

Create enough manually reviewed cases to exercise semantic distinctions rather than token overlap. Add generated variants only when provenance/grouping prevents train/test leakage.

Include an unknown-tool suite in which candidate names are synthetic/withheld and descriptions carry the semantics.

## 7. Ordered work packages

### Work package A — Data-contract tests first

Add schema round-trip, validation, stable version, duplicate-candidate, missing-label, and invalid-`none` tests.

Acceptance evidence: malformed cases are rejected with actionable diagnostics.

### Work package B — Curated seed corpus

Build reviewed cases from current CodeGG tools and representative coding prompts. Tag hard-negative families and no-tool cases.

Acceptance evidence: fixture validator reports counts by domain/tag and no duplicate IDs/groups.

### Work package C — Unknown-tool and leakage guards

Create deterministic holdout transforms or dedicated fixtures that rename/withhold tools while preserving descriptor semantics.

Acceptance evidence: a name-memorizing baseline cannot trivially pass the unknown-tool suite.

### Work package D — Existing-search baselines

Adapt the current keyword/BM25 implementation for offline candidate ranking without creating a second algorithm.

Acceptance evidence: benchmark emits machine-readable and human-readable results for keyword and BM25.

### Work package E — Documentation and reproducibility

Document fixture format, split policy, metric definitions, and how to add cases.

## 8. Failure, cancellation, restart, and contention semantics

No long-running production work is added. Benchmark commands may fail on invalid data but must not mutate user/session state. Interrupted benchmark runs can simply restart.

## 9. Compatibility and migration

The first case schema is `v1`; later incompatible changes require explicit migration/version readers or a deliberate fixture rewrite documented in planning/closure. No user database migration.

## 10. Required tests

### Focused unit tests

- case validation and serialization;
- deterministic split assignment;
- metric correctness on small hand-computed examples;
- unknown-tool transform behavior;
- BM25/keyword adapter determinism.

### Integration tests

- load the repository fixture corpus;
- validate every candidate/label;
- produce stable baseline summary within deterministic floating-point tolerances.

### Security and negative tests

- benchmark loaders reject unbounded/oversized fields;
- path arguments cannot escape the configured fixture root if file loading is exposed;
- benchmark code does not trigger network access.

## 11. Required verification commands

Use exact targets created by implementation. Expected minimum:

```bash
cargo test -p codegg --lib tool::catalog
cargo test -p codegg --lib tool::tool_search
cargo test -p codegg --lib tool_advisor
cargo test --test tool_surface_minimization
cargo run --locked -- tool-advisor bench --dataset <repo-fixture-path>
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

If the CLI is intentionally dev-only rather than the main command tree, use the implemented equivalent and record it in closure.

## 12. Documentation updates

- New architecture note for tool-advisor data/evaluation contracts.
- `architecture/agent-tool-surface.md` cross-reference explaining that benchmarking consumes, but does not alter, discovery.
- Fixture contributor documentation.

## 13. Acceptance criteria

M001 closes only when:

1. a versioned benchmark case/prediction schema exists;
2. curated hard-negative, no-tool, multi-tool, and unknown-tool cases are present and validated;
3. deterministic split/holdout logic prevents obvious leakage;
4. keyword and BM25 baselines are reproducible from Rust;
5. normal CodeGG runtime behavior and dependency graph are unchanged;
6. no network or ML runtime is required;
7. focused/broad verification is recorded.

## 14. Stop conditions

Stop rather than improvise if:

- useful labels require changing tool authority/disclosure semantics;
- a proposed fixture format would embed secrets or real private repository data;
- baseline evaluation requires a second independent implementation of ToolCatalog ranking;
- scope expands into learned inference/training/telemetry.

## 15. Closure evidence required

- schema/version documentation;
- corpus counts and provenance categories;
- hard-negative and unknown-tool coverage table;
- exact keyword/BM25 metrics;
- proof default runtime/dependency graph did not gain ML/network behavior;
- exact test/verification output;
- residual dataset gaps ranked by severity/value.

## 16. Handoff notes

Do not optimize the dataset to make the future model look good. M001 exists to make failure visible. Prefer a smaller reviewed corpus with difficult semantic distinctions over thousands of templated near-duplicates.
