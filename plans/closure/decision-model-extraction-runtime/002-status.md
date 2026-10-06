# Decision-Model Extraction and Runtime Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/decision-model-extraction-runtime/002-external-training-runtime-extraction.md`

Source subsystem roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m002--external-trainingruntime-repository-extraction-contract`

Repository baseline reviewed: `533be5941ac48334743de555cda96d8695cc6527`

CodeGG source/provenance revision: `aec30ebbaada172cd2915b3fb068f828a8fc6709`

External destination: <https://github.com/dbowm91/sdm>

Destination revision: `33a7f4a0a404755a5a61f01731eb7bdfdd81d4dd`

License/release policy: MIT with preserved Anomalyco notice and source provenance; initial
repository visibility is private. Consumers pin immutable full commit SHAs after CI and
compatibility qualification. This revision is the first pin. It contains no release tag;
the commit is the immutable runtime revision for M003. The artifact is a smoke baseline,
not a qualified production model.

## 1. Executive finding

M002 is complete. SDM independently owns semantic primitives, local Rank artifact
execution, generic decision-case dataset handling, and a deterministic training and
evaluation baseline. `sdm-runtime` depends on `sdm-core` and runtime libraries only; it
does not depend on `sdm-training`, Python, an HTTP client, or a model-download facility.
The extraction is additive: CodeGG's live advisor, feature flags, dependencies, causal
frontier, capture policy, fixture authority, and historical experiment evidence remain
unchanged. M003 is unblocked and registered as ready.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Source inventory classifies every `src/tool_advisor/*` file | `sdm/docs/extraction-inventory.md` | pass | Includes file-by-file classification and retains causal-frontier contracts/state explicitly. |
| Relevant scripts, assets, features, and dependencies classified | `sdm/docs/extraction-inventory.md` | pass | Frozen CodeGG records stay authoritative; only the M001 compatibility fixture copy and its derived small Rank cases are exported. |
| Destination has independent decision core, local runtime, training/evaluation roles | SDM workspace `sdm-core`, `sdm-runtime`, `sdm-training` | pass | Runtime does not depend on training. |
| License and provenance are preserved | SDM `LICENSE`, `README.md`, `docs/extraction-inventory.md` | pass | MIT notice preserved; CodeGG source file/commit and copied fixture source commit recorded. New runtime/trainer are identified as new implementations. |
| Generic dataset contract covers Binary/Choice/Score/Rank and multi-relevance Rank | `sdm-training::DecisionCase`, `sdm-core` semantic types | pass | Dataset validator accepts the full M001 request/response contract. Initial trainer fits Rank relevance only; semantic core preserves all kinds and unsupported semantics remain explicit. |
| Training is reproducible, split-safe, calibrated off final test, and reports resources | `sdm-training::train`, `evaluate`; `docs/reproducibility.md` | pass | Seeded leakage-group SHA splits; train-only fitting; dev-only threshold selection; explicit held-out evaluation; report includes split counts, epochs, parameter count, artifact bytes/digests. |
| Local runtime validates manifest/artifact, bounds input, exposes capability and never downloads | `sdm-runtime`; `docs/artifact-format.md` | pass | Fail-closed schema, architecture, capability, provenance, model/calibration hashes, limits and finite parameters; local bytes only. |
| Versioned compatibility artifact trains independently of CodeGG | `fixtures/compatibility-rank-cases-v1.jsonl`, `artifacts/compatibility-rank-v1.json` | pass | Reproduced with seed 127 and 100 epochs; report: 2 train / 1 dev / 1 test case, 62 parameters, 2560 bytes, digest `sha256:b9484ce8b8afe2b345693e084468c89a628462e86faf1e9683c28ee53de6597e`. |
| M001 fixtures pass under declared compatibility tolerance | `scripts/check_sdm_compatibility.sh`; SDM `sdm-check-compat` | pass | 12 fixtures parsed and expected responses validated; 11 valid requests executed. Runtime outputs validate under CodeGG semantics, preserve Rank candidate identities when answering, and carry artifact/runtime provenance. Frozen advisor labels remain quality evidence, not an assertion that this unqualified baseline reproduces predictions. |
| CodeGG production behavior and historical evidence unchanged | CodeGG diff and source inventory | pass | No production runtime, config, capture, causal-frontier, or old training implementation changed. No frozen historical evidence was rewritten. |

## 3. Extracted destination contents

The external workspace contains:

- `sdm-core`: M001 bounded Binary, Choice, Score, Rank, status, provenance, validation,
  fingerprint, capability and `DecisionEngine` primitives copied with source attribution.
- `sdm-runtime`: offline manifest-validated sparse Rank inference, independent candidate
  relevance, abstention, artifact/model/calibration digest checks, and explicit unsupported
  responses for other model semantics.
- `sdm-training`: generic bounded JSONL decision cases, duplicate/schema validation,
  deterministic leakage-group splits, sparse logistic Rank fitting, dev-only abstention
  threshold selection, held-out evaluation, and machine-readable resource/provenance report.
- `fixtures/`: the unchanged, sanitized M001 fixture set plus derived training cases. No
  CodeGG user session or capture data was copied.
- `docs/`: ownership, extraction inventory, artifact format, compatibility tolerance,
  provenance, license, reproducibility, and immutable revision pin policy.

The CodeGG-side runner accepts an SDM checkout and expected full SHA, rejects a different
HEAD, and invokes the external fixture binary against SDM's copy of CodeGG's frozen
fixtures. This keeps exact cross-repository revision evidence reproducible without adding
SDM to CodeGG's production dependency graph.

## 4. Verification executed

### SDM commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo check -p sdm-runtime --locked
cargo tree -p sdm-runtime --locked
python3 scripts/import-codegg-compat-fixtures.py
cargo run --locked -p sdm-training --bin sdm-train -- \
  fixtures/compatibility-rank-cases-v1.jsonl artifacts/compatibility-rank-v1.json 127 100
cargo run --locked -p sdm-training --bin sdm-check-compat -- \
  fixtures/codegg-compatibility-v1.jsonl artifacts/compatibility-rank-v1.json
```

Results: format, Clippy, runtime-only check, and workspace tests passed (9 unit tests total;
doc tests passed). Runtime dependency tree contains no `sdm-training` package or ML
framework. The trainer reproducibly produced the pinned artifact; the checker validated
all 12 fixtures and executed 11 valid cases. No fixture was silently skipped as valid.

### CodeGG command

```bash
scripts/check_sdm_compatibility.sh /tmp/sdm \
  33a7f4a0a404755a5a61f01731eb7bdfdd81d4dd
```

Result: passed; exact revision guard matched and the SDM checker validated 12 fixtures
(11 runtime executions) against that immutable checkout.

CodeGG `cargo fmt`, `scripts/verify.sh quick`, and `git diff --check` are run for the
CodeGG planning/documentation/script update before its closure commit.

## 5. Invariant and failure review

- No user data was exported; only frozen sanitized compatibility cases were copied.
- CodeGG `ResolvedToolSurface`, permissions, disclosure/promotion budget, causal frontier,
  consent, actuation, and deterministic fallback remain CodeGG-owned.
- Compatibility output remains advisory and has no production CodeGG caller.
- Runtime artifact loading uses only provided local bytes; malformed/version-mismatched
  artifacts fail closed and never trigger an implicit fetch.
- A failed SDM checkout/build/check leaves the CodeGG runtime and frozen evidence intact.
- CodeGG retains its old model code until M003/M006; no authority moved cross-repository.

## 6. Qualification boundary and remaining work

The compatibility artifact demonstrates independent training, bounded local loading, and
M001 semantic compatibility. It is not a predictive-quality qualification, does not
promote a previously negative CodeGG model, and is not enabled in production. M003 must
own model-runtime adoption, shadow/parity/fallback evidence, and artifact resource review.
M005 remains blocked until M003 closes. M006 remains blocked until the migration and a
fresh production call-site/dependency inventory are complete.

## 7. Unblocked work

- M003 is ready with the exact SDM revision and artifact digest recorded in
  `plans/registry.md` and the subsystem roadmap.
- M005 remains blocked on positive M003 evidence; M004's independent second-backend
  contract is already closed and stable.
- M006 remains blocked on M005 and the required final legacy reachability inventory.
