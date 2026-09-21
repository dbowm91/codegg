# Tool-Selection Advisor Sequence-Encoder Experiment M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/004-sequence-encoder-ranking-experiment.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m003--true-sequence-encoder-ranking-experiment`

Repository baseline reviewed: `1633bfce`

Implementation commits:

- `bf3e4b5` — advisor: implement sequence ranking retrieval qualification harness

## 1. Executive finding

M003 closes positively with the packed-marker, mean-pooled, frozen-encoder
head-only artifact selected for downstream retrieval work. On the frozen C001
train/dev partitioning, the selected ranker reached dev MRR 0.823 and Recall@1
0.806, versus BM25 dev MRR 0.515. Pairwise cross-encoder head-only reached dev
MRR 0.595. Both artifacts load from the explicit local MiniLM manifest and
carry source/config/tokenizer/license hashes, partition fingerprints, stage,
calibration, and final head hashes.

The top-layer stage was attempted through the M001B differentiable path after
stage-1 signal, but did not produce an artifact within ten minutes on the
development Mac CPU. That measured cost is recorded as a resource stop, not a
quality claim; full fine-tuning was not justified. The head-only result is
credible and unblocks M004, whose own retrieval/resource gates remain open.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Pairwise ranker | `sequence_ranking.rs`; actual MiniLM run | pass | Dev MRR 0.595, scalar relevance plus explicit abstention head. |
| Packed marker ranker | `sequence_encoder.rs` packed layout + ranker | pass | Dev MRR 0.823; marker IDs are existing `[unusedN]` vocabulary entries. |
| Frozen/head-only training | M003 pairwise and packed JSON reports | pass | Deterministic seed 17; mean pooling selected explicitly. |
| Dev-only calibration | `best_abstention_threshold` and artifact calibration fields | pass | No final-test labels used during training or stage selection. |
| M001B unfreeze path | top-layer attempt used `DifferentiableBertModel` | partial | Framework path is wired; local cost stop prevented a completed top-stage artifact. |
| Artifact contract | `SequenceArtifactManifest`, safetensors head, hash validation | pass | Encoder/source/license/config/tokenizer and final-head hashes recorded. |
| Context/data discipline | `AdvisorContextV2::from_benchmark_context`; C001 fingerprints | pass | Train 132, dev 62, test 62; test labels were not read by M003. |
| Local/default isolation | feature-gated modules and CLI | pass | Default build has no Candle/model dependency or download path. |

## 3. Production implementation evidence

The implementation is experiment-only. Pairwise scoring builds bounded
`[CLS] context [SEP] descriptor [SEP]` inputs. Packed scoring records token
budget, accepted marker positions, and dropped candidates. `SequenceRankingHead`
owns relevance and abstention logits; stage selection scopes the optimizer to
the head, top layers, or full differentiable encoder. Artifacts are explicit
local files and are never loaded by the production advisor runtime.

## 4. Verification executed

```bash
rtk cargo fmt --all
rtk cargo check --locked --features tool-advisor-encoder-training -p codegg
rtk cargo test --locked --features tool-advisor-encoder-training -p codegg --lib
rtk cargo run --locked --features tool-advisor-encoder-training --bin codegg -- tool-advisor sequence-encoder-rank --config assets/tool-advisor/sequence-ranking-minilm.json --json
rtk cargo run --locked --features tool-advisor-encoder-training --bin codegg -- tool-advisor sequence-encoder-rank --config assets/tool-advisor/sequence-ranking-minilm-packed.json --json
rtk git diff --check
```

Results: the feature-gated library suite passed 4,913 tests after the final
M003 implementation fixes; formatting, feature check, and diff checks passed.
The pairwise run reported dev MRR 0.5954; the packed run reported dev MRR
0.8226, Recall@1 0.8065, no-tool F1 0.6667, and zero packed drops. The
top-layer attempt was stopped at 12 minutes without an artifact and is the
named local resource condition above.

## 5. Invariant review

- AdvisorContextV2 remains bounded, versioned, and privacy-filtered.
- The encoder only scores textual candidates; it does not grant authority,
  execute tools, or construct arguments.
- No fixed classifier IDs or remote teacher calls were introduced.
- Final test/family-holdout labels remain outside M003 model selection.
- The default build remains model-free, network-free, and feature-isolated.

## 6. Failure and recovery review

Malformed configs, missing local assets, hash drift, unsupported stages,
non-finite losses, and shape mismatches fail closed. Packed overflow reports
dropped candidates. The top-layer resource stop produced no artifact and did
not alter the selected head-only artifact. No persistent user-context cache or
restart-sensitive state was introduced.

## 7. Migration and compatibility review

The change is additive and experiment-gated. Existing linear/contextual
artifacts remain loadable through their existing paths. New sequence artifacts
have a distinct schema and architecture identifier; no old artifact is
reinterpreted and no storage migration is required.

## 8. Security review

Asset loading remains explicit-path, hash-checked, and offline. Packed and
pairwise inputs contain only bounded AdvisorContextV2 and candidate descriptor
text. Retrieval cache work is downstream and deferred-only; M003 itself adds
no authority or telemetry surface.

## 9. Documentation and operations

`architecture/tool-advisor-framework-spike.md` records the selected variant,
resource stop, and experiment boundary. The rank/retrieval/qualification CLI
commands and JSON configs document reproducible local operation.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Top-layer/full fine-tuning is not qualified within the local CPU resource bound | Frozen head-only selection remains valid; deployment cost is not hidden | M004/M005 use the selected head-only artifact; revisit only under a separately budgeted plan. |
| low | Only the qualified ~22M MiniLM asset was available | No second real capacity point was available; no synthetic capacity claim was made | Add a separately qualified real asset before any capacity comparison. |

No high or medium findings remain.

## 11. Roadmap disposition

M003 is closed with a positive experimental result. M004 is unblocked and
ready because M003's selected encoder/tokenizer/head contract is stable. M005
remains downstream of M004. The existing live-primary-model M004 remains
blocked pending positive M005 qualification.

## 12. Registry updates

- M003 moves from `ready` to recently closed with this record and `bf3e4b5`.
- M004 moves from blocked to dependency-ready `ready` in the same closure
  commit; its remaining acceptance gate is hybrid recall/resource evidence.
- M005 remains blocked on M004 and is not silently unblocked.
- No corrective plan is registered: the top-layer resource stop is a bounded
  experiment disposition, not an implementation correctness defect.
