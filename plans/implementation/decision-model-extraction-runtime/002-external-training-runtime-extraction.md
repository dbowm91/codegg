# Decision-Model Extraction and Runtime Milestone 002 — External Training/Runtime Extraction

Status: blocked

Repository baseline: `533be5941ac48334743de555cda96d8695cc6527`

Source roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m002--external-trainingruntime-repository-extraction-contract`

Applicable ADRs:

- `plans/adrs/ADR-0013-external-decision-model-training-and-runtime-boundary.md`
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` for retained privacy/evidence invariants.

Primary class: infrastructure

Hard dependencies:

- M001 closed.

Operational/interface dependency:

- a maintainer-approved external decision-model repository exists and is writable/available to the implementation agent.

## 1. Objective

Extract generic decision-model training, evaluation, artifact, and reusable local-runtime ownership from CodeGG into the external decision-model repository, then prove that repository against the frozen CodeGG M001 compatibility fixtures.

This is a cross-repository migration milestone. CodeGG remains the source of historical evidence and the downstream compatibility consumer; the external repository becomes the owner of generic learned-model implementation.

## 2. Why this milestone is blocked

The exact external repository does not yet exist in CodeGG planning and ADR-0013 deliberately does not invent its permanent name.

Implementation may begin only after:

- M001 freezes the application contract/fixtures;
- maintainers provide or create the destination repository;
- destination license/release policy is explicit.

No model code should be deleted from CodeGG while the destination is unspecified.

## 3. Extraction classification

Before moving code, produce a source inventory classifying every `src/tool_advisor/*`, relevant script, asset, feature, and dependency as one of:

- **generic runtime/training — extract**;
- **CodeGG adapter/policy — retain**;
- **historical evidence — retain in place**;
- **obsolete experiment implementation — retain until M006, then retire**;
- **shared utility requiring a deliberate split**.

Expected generic candidates include portions of:

- local training/calibration/optimizer/checkpoint machinery;
- sequence encoder loading/inference;
- generic ranking heads;
- generic projection/late-interaction math;
- artifact manifest/hash/provenance handling;
- generic benchmark/calibration metrics.

Expected CodeGG-owned material includes:

- `ResolvedToolSurface` projection and authority tests;
- tool-specific context construction;
- disclosure/promotion policy;
- training-event capture from CodeGG sessions;
- causal-frontier contracts/state/evaluation;
- downstream coding trajectory qualification.

Do not treat file location as ownership proof; split mixed files deliberately.

## 4. Destination repository contract

The external repository must establish separable package/module roles, regardless of final names:

1. **decision core** — semantic/task/artifact-neutral primitives required by runtime/training;
2. **local runtime** — load validated artifact, expose capabilities, evaluate bounded requests;
3. **training/evaluation** — datasets, splits, training loops, calibration, experiment harnesses;
4. optional architecture modules behind explicit features.

The local runtime must be consumable by CodeGG without importing training dependencies.

Training is free to use Rust or non-Rust tooling, but the runtime consumed in CodeGG must satisfy the accepted CodeGG integration contract.

## 5. Required external-repository work

### Repository provenance

Record the CodeGG source baseline and source paths for extracted implementation. Preserve MIT license attribution and enough history/provenance to distinguish moved CodeGG-origin code from new work.

A filtered history is optional; clear source-commit provenance is required.

### Generic dataset contract

Define a generic bounded decision-case representation supporting at least Binary/Choice/Score/Rank and multiple relevant Rank candidates.

CodeGG-specific tool names/fields are fixtures/plugins, not baked into the generic schema.

### Training/evaluation

Port or rewrite useful training/evaluation machinery rather than copying every failed experiment wholesale.

The destination must support:

- deterministic seed/split recording;
- train/dev/test separation;
- leakage-group metadata;
- calibration selected outside final test;
- unknown/renamed candidate evaluation where relevant;
- artifact provenance and license metadata;
- resource reporting.

Historical negative architectures may be retained as baselines, but are not required as supported runtime architectures.

### Local runtime

Expose a Rust library/API with:

- artifact validation;
- semantic capability declaration;
- bounded input validation;
- deterministic inference for a pinned artifact;
- explicit unsupported/unavailable errors;
- no implicit model download;
- no training dependencies in minimal runtime features.

The runtime may use architecture-specific internal code; CodeGG must not.

### Artifact manifest

At minimum encode:

- manifest schema;
- runtime/architecture id;
- semantic capabilities;
- tokenizer/input schema version when used;
- input/candidate limits;
- weight/model hash;
- calibration version/hash;
- training/evaluation provenance fingerprint;
- license/source metadata.

## 6. CodeGG-side work

- Export or vendor the M001 compatibility fixtures in a machine-readable form the external repo can consume, without moving the authoritative CodeGG copy.
- Add a compatibility test command/script that can run an external runtime binary/library fixture output against CodeGG expected semantics.
- Document the external repository URL/release once it exists.
- Do not switch production runtime ownership yet.
- Do not remove legacy features/dependencies yet.

If a temporary git/path dependency is required solely for compatibility testing, keep it out of the default production graph and remove it when M003 adopts a released/pinned dependency.

## 7. Ordered work packages

### A — Extraction inventory

Classify all current learned-advisor source/assets/dependencies and review the boundary.

Acceptance: every file/module named in `src/tool_advisor/` is classified; causal-frontier code is explicitly retained.

### B — Destination bootstrap and provenance

Create/populate the external repository with license, CI, runtime/training separation, and CodeGG source provenance.

Acceptance: independent checkout builds/tests without CodeGG.

### C — Generic training/evaluation migration

Move or reimplement the useful generic pipeline around the generic decision schema.

Acceptance: at least one small compatibility model/baseline can train/evaluate without a CodeGG checkout.

### D — Runtime/artifact contract

Implement the minimal local runtime and versioned artifact loader.

Acceptance: runtime-only build excludes training frameworks not needed for inference and performs no implicit download.

### E — Cross-repo compatibility qualification

Run the external runtime over M001 fixtures and compare semantic outputs/status/provenance under the declared tolerance policy.

Acceptance: all contract fixtures pass, including unsupported/malformed cases.

### F — Release/pin evidence

Produce a commit/tag/revision CodeGG can consume in M003. Record exact source revision and compatibility version.

## 8. Failure, restart, and contention

Training interruption/checkpoint behavior belongs to the external repository and must be documented there.

Cross-repo extraction must be additive until M003/M006: a failed destination build or release leaves CodeGG unchanged.

No concurrent write path may make both repositories authoritative for the same installed artifact. Artifact production belongs to the external project once M002 closes.

## 9. Security and privacy

- No user CodeGG data is copied during extraction.
- Frozen repository fixtures may be copied only if they contain no secrets/private session content and their provenance permits it.
- The external trainer never scans CodeGG state directories implicitly.
- Runtime artifact validation is fail-closed.
- Network/model download is never implicit.

## 10. Required tests and verification

Destination repository:

- unit tests for generic semantic schemas;
- train/eval split/leakage tests;
- artifact tamper/version tests;
- runtime-only dependency/features check;
- deterministic fixture inference;
- CI on its supported training/runtime platforms.

CodeGG:

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
git diff --check
```

plus the M001 compatibility checker against the exact external runtime revision.

## 11. Documentation updates

CodeGG:

- `architecture/tool-advisor.md`
- `architecture/tool-advisor-framework-spike.md` as historical context, clearly marking framework/model research ownership moved;
- roadmap/registry dependency state.

External repository:

- architecture/ownership boundary;
- artifact format;
- training/evaluation reproducibility;
- CodeGG compatibility fixture instructions.

## 12. Acceptance criteria

M002 closes only when:

1. a real external repository owns generic training/evaluation/local-runtime work;
2. its runtime can be built independently of CodeGG;
3. training can run independently of CodeGG;
4. the runtime passes M001 compatibility fixtures;
5. a versioned artifact/runtime revision is available for M003;
6. CodeGG production behavior is still unchanged;
7. CodeGG historical evidence remains intact.

## 13. Stop conditions

Stop if:

- destination ownership is ambiguous;
- extraction requires moving `ResolvedToolSurface`, permissions, promotion budgets, or causal contracts;
- runtime-only consumers inherit training/Python dependencies;
- historical evidence would need to be rewritten;
- compatibility can be obtained only by weakening M001 semantics;
- the external repository silently downloads model assets.

## 14. Closure evidence required

- destination repository URL/revision/tag;
- extraction inventory;
- provenance/license record;
- runtime dependency graph;
- artifact manifest example;
- M001 compatibility report;
- training/evaluation smoke evidence;
- CodeGG verification evidence;
- unresolved cross-repo findings and owner;
- dependency audit unblocking M003.
