# Tool-Selection Advisor Sequence-Encoder Experiment M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/006-clean-offline-sequence-encoder-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m005--clean-offline-sequence-encoder-qualification`

Repository baseline reviewed: `0134f3982b17f6d4f594e37b5560048a1bfad0a0`

Implementation commits:

- `bf3e4b5` — advisor: implement sequence ranking retrieval qualification harness
- `2b1b0b2` — plans: freeze M005 qualification protocol
- `ad98cf8` — plans: finalize M005 protocol encoding
- `0134f39` — plans: record M005 preregistration commit
- `040ed8e` — advisor: satisfy experiment clippy checks

## 1. Executive finding

M005 closes with disposition **D — no useful gain** for live promotion. The
frozen packed-marker, mean-pooled MiniLM ranker was materially better than the
hashed-linear aggregate test baseline (MRR 0.8306 versus 0.7070), and the RRF
retriever recovered all relevant tools on both deterministic 64- and 128-tool
fixtures. However, the preregistered contextual-slice gate could not pass
because no contextual comparison artifact was declared, and the selected
resource gate failed cold load at 16.723 seconds against a 10-second limit.
The implementation and evidence remain research-only; the existing live
primary-model M004 remains blocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Separate preregistration freeze | `assets/tool-advisor/sequence-encoder-m005-preregistration.json` | pass | Dataset, partitions, model files, training configs, retrieval policy, budgets, command, gates, and resource limits are hashed/frozen. |
| Baseline comparison arms | `assets/tool-advisor/sequence-qualification-result.json` | pass | Keyword, BM25, hashed-linear-v1, rejected contextual label, selected sequence ranker, hybrid retrieval label, and no-advisor disclosure are reported. |
| Zero leakage | result `zero-leakage` gate | pass | Exact, normalized, and template-family cross-split overlaps are all zero. |
| Authority-negative safety | result `zero-authority-negative-promotions` gate | pass | Zero unauthorized retrieved descriptors. |
| Expanded retrieval recall | result `candidate-recall-64` and `candidate-recall-128` gates | pass | Both fixtures recovered 1.0000 recall at RRF K=16. |
| Aggregate quality | result `aggregate-mrr-vs-linear` gate | pass | Sequence MRR 0.8306; hashed-linear MRR 0.7070. |
| Contextual-slice gain | result `contextual-slice-gain` gate | fail | No contextual artifact was preregistered; this was not silently treated as a pass. |
| Resource limits | result `resource-budget` gate | fail | Cold load 16,723ms/10,000ms; test rank 75,816ms/600,000ms; weights 90,868,376B/134,217,728B. |
| Offline/default isolation | feature-gated implementation and exact offline command | pass | No provider call, network path, runtime model download, or live advisor wiring was introduced. |

## 3. Production implementation evidence

The qualification command is experiment-gated and offline-only. It verifies
the frozen dataset and partition fingerprints, selected sequence artifact,
encoder manifest/config/tokenizer/source-weight hashes, deterministic hybrid
retrieval policy, and resource limits before producing the result. The
retriever remains deferred-only, authority-preserving, cache-invalidating on
surface changes, and BM25-fallback-safe. No live disclosure path consumes the
sequence artifact.

## 4. Verification executed

The exact frozen evaluation command was:

```bash
cargo run --locked --features tool-advisor-encoder-training --bin codegg -- \
  tool-advisor sequence-encoder-qualify \
  --prereg assets/tool-advisor/sequence-encoder-m005-preregistration.json --json
```

It produced:

- sequence test MRR `0.8306451613`, Recall@1 `0.8064516129`, Recall@3
  `0.8548387097`, no-tool F1 `0.5000`, and zero dropped candidates;
- 124 sequence forwards and 75,816ms total test ranking time;
- RRF K=16 retrieval recall `1.0000` on the 62-case test slice, the 64-tool
  fixture, and the 128-tool fixture;
- warm descriptor cache entries `23`, qualification cache entries `25`, and
  no cached context;
- calibration Brier `0.1742529468`, ECE `0.5698638635`, and NLL
  `0.5389472189` for the selected sequence artifact.

The result is committed as:

- `assets/tool-advisor/sequence-qualification-result.json`

Hosted CI was available. At closure preparation, GitHub Actions runs for the
freeze-chain commits were in progress, including run `35671296266` for the
final preregistration-provenance commit; their final conclusions are checked
before the closing push is reported.

## 5. Invariant review

- C001 dataset, train/dev/test fingerprints, and final labels remain frozen.
- The qualification path reads final test labels only after all artifact and
  protocol hashes validate.
- Retrieval considers only already-authorized deferred candidates.
- No advisor output grants permission, executes tools, or constructs arguments.
- No user context is persisted in the descriptor cache.
- Normal/default builds remain independent of model weights and network access.
- The negative disposition does not wire experimental artifacts into live
  primary-model behavior.

## 6. Failure and recovery review

Malformed preregistration, protocol-hash drift, partition drift, encoder asset
drift, sequence artifact drift, invalid retrieval mode, and resource-budget
overruns fail closed or produce a non-qualifying disposition. The initial
protocol-hash encoding correction was made before any final labels were read;
the subsequent exact run used the corrected canonical digest. No model,
dataset, gate, or final-test result was changed after the successful frozen
evaluation began.

## 7. Migration and compatibility review

M005 is additive and experiment-gated. It introduces no persistent runtime
state, storage migration, provider contract, or default advisor behavior.
The result and preregistration files are repository-owned evidence; the
pretrained weights remain an explicit ignored local asset.

## 8. Security review

The selected encoder and retrieval artifacts have no network or download path.
Manifest hashes, source-weight hashes, tokenizer hashes, and explicit local
paths are checked. The authority-negative test returned zero violations, and
the retriever cannot introduce hidden, denied, core, or non-deferred tools.

## 9. Documentation and operations

The sequence experiment architecture note documents the selected packed-marker
variant, retrieval boundary, resource stop, and offline-only disposition. The
preregistration records the exact command, selected RRF K=16 policy, promotion
limits, schema budget, and resource ceilings. The machine-readable result is
the reproducibility receipt for the final run.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No contextual comparison artifact was available for the final preregistration | The contextual-slice gain gate cannot qualify a live model | A future architecture experiment must provide a declared contextual arm before any positive claim. |
| medium | Cold load exceeded the declared 10s limit | The selected ranker is not deployment-qualified on this target | Retain research-only; any optimization requires a new preregistration and qualification. |
| low | Resource report does not include Apple-accelerated latency, RSS, or binary delta | Cross-target deployment cost is not characterized | Measure only under a separately scoped, hardware-specific qualification. |
| low | No separate family-holdout contextual metrics were emitted by this protocol | The negative disposition remains conservative rather than a positive generalization claim | Do not treat aggregate MRR/retrieval recall as live qualification. |

These are explicit reasons for disposition D, not blockers to closing M005.
No corrective plan is registered inside this workstream because live
qualification is intentionally blocked and the next model experiment would
need a new scope, preregistration, and evidence contract.

## 11. Roadmap disposition

M005 is closed with disposition D. The sequence-encoder experiment workstream
is closed. No future plan in this sequence-encoder workstream can be
unblocked by this result: the only downstream live-primary-model M004 requires
a positive offline disposition, and the result is negative. Existing M001A
and M001B records remain historical evidence; they are not reopened.

## 12. Registry updates

- Plan 006 moves from `ready for handoff` to `implemented`.
- The sequence-encoder roadmap moves from `active` to `closed` with M005
  disposition D.
- M005 is removed from dependency-ready plans and recorded as closed here.
- The existing live-primary-model M004 remains explicitly blocked; no future
  plan is marked ready as a consequence of this negative result.
- The result artifact, preregistration, roadmap, registry, and this closure
  record are committed together in the closing commit.
