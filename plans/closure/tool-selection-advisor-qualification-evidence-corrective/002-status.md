# Tool-Selection Advisor Qualification Evidence Corrective C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-qualification-evidence-corrective/002-fresh-v3-preregistered-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-qualification-evidence-corrective-addendum.md#c002--fresh-v3-preregistered-requalification`

Repository baseline reviewed: `a744eae2`

Implementation and evidence commits:

- `4ef947c` — advisor: implement qualification evidence corrective c001
- `83405ff8` — plans: close qualification evidence corrective c001
- `a744eae2` — plans: freeze c002 v3 preregistration (Commit A)
- Commit B (this closure): v3 result, closure record, registry disposition

Qualification result:

- `assets/tool-advisor/sequence-qualification-v3-result.json`
- `a744eae294778d320ffee4f759de5c2bcf5fc3e3` — Commit A SHA echoed by the result
- disposition: **D — no useful quality gain**

## 1. Executive finding

C002 closes with one valid release-mode qualification of the unchanged
selected MiniLM packed-marker artifact against the semantically valid v3
holdout under the corrected retrieval identity semantics. Protocol,
artifact, partition, manifest, leakage, authority, calibration, and
resource checks all passed; the ranker failed the preregistered quality,
generalization, retrieval, and promotion requirements. This is an honest D
disposition — substantive negative evidence about the current ranker on a
valid corpus, not another harness artifact.

The single final run succeeded on its first execution; no retry was needed
and none occurred. No result existed before the run recorded here.

## 2. Preregistration and run receipt (Commit A)

| Item | Frozen evidence | Result |
|---|---|---|
| Protocol | `c002-preregistered-release-sequence-qualification-v3` | pass |
| Preregistration commit | `a744eae294778d320ffee4f759de5c2bcf5fc3e3` | echoed in result |
| Protocol hash | `e46286568ba37088b3849a24ffd2fb67b9cf573569e49583f816897a8dfdf6bc` | pass |
| Historical dataset | `06da7e530799df915c12ceaefe1076e40b70047c6f4276f37d70685e2c2d3582` | pass |
| Historical train/dev/test | `c67df3ca…` / `b804b7d8…` / `1765ad09…` | pass |
| Fresh v3 holdout | `fafdecb5a9d91e368e81e686ea5afc9d4232cff04101904e7b79974b126a1c54` (170 cases) | pass |
| Fresh v3 manifest | `54a3f47b7ecb31275a35da9d2d9864c6bec44a25163c2f6b43f0f4edcbbad065` | pass |
| Selected sequence artifact | `01b5c368b4e762dbe6ca284694b290b72e17ec64606e1f26a21510721f9a10e8` | pass |
| Encoder manifest/config/tokenizer/source | `a74faa95…` / `953f9c0d…` / `07eced37…` / `53aa5117…` | pass |
| Linear baseline | `target/tool-advisor-smoke/model.json` → `20a7927e…` | pass |
| Retrieval | rrf, shortlist K=16, universes [64, 128] | pass |
| Promotion | dev threshold `0.4782007038593292`, cap 2, budget 16 KiB | pass |
| Target | Apple Silicon macOS (M4 Pro), CPU device; filesystem cache uncontrolled | reported |
| Final command | `./target/release/codegg tool-advisor sequence-encoder-qualify-v3 --prereg assets/tool-advisor/sequence-qualification-v3-preregistration.json --prereg-commit a744eae294778d320ffee4f759de5c2bcf5fc3e3 --json` | executed once |

Hosted CI for `a744eae2` passed as GitHub Actions run `35697246685`
before the final model evaluation, satisfying the preregistration gate.
Commit A contains the preregistration JSON plus one additive
hash-regression unit test (test-only, no production delta); the release
binary, artifact, holdout, gates, and measured paths are byte-identical to
the C001 closure state.

One-run discipline observed: clean tree at Commit A; protocol, artifact,
baseline, and v3 hashes revalidated; C001 semantic validators green;
exact release binary built from Commit A; one qualification executed;
result and closure committed without changing inputs.

## 3. Holdout integrity and leakage evidence

| Measure | Value | Gate |
|---|---:|---|
| Cases | 170 | pass |
| Leakage groups | 157 | pass |
| Skeletons | 157 (max share 1.2%) | pass |
| Counterfactual pairs | 13 | pass |
| Exact overlap | 0 | pass |
| Normalized overlap | 0 | pass |
| Template overlap | 0 | pass |
| Explicit family overlap | 0 | pass |
| Copied-context overlap | 0 | pass |

Required slice floors were met: hard-negative 28, counterfactual 26,
unknown-renamed 24, no-tool 24, multi-tool 24, context-v2-long-session 28,
plugin/MCP 22, LSP 26, research/search 34, structured/data 38.

## 4. Quality and slice evidence

The complete metric payload is in the machine-readable result. Every
declared slice's cases, frozen-linear MRR/Recall@1, and sequence
MRR/Recall@1:

| Slice | Cases | Linear MRR | Sequence MRR | Linear R@1 | Sequence R@1 |
|---|---:|---:|---:|---:|---:|
| aggregate | 170 | 0.7095 | 0.2613 | 0.6000 | 0.0000 |
| hard-negative | 28 | 0.7738 | 0.3036 | 0.5714 | 0.0000 |
| counterfactual | 26 | 0.6135 | 0.2821 | 0.3846 | 0.0000 |
| unknown-renamed | 24 | 0.9028 | 0.3264 | 0.8333 | 0.0000 |
| no-tool | 24 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| multi-tool | 24 | 1.0000 | 0.3333 | 1.0000 | 0.0000 |
| context-v2-long-session | 28 | 0.9524 | 0.3095 | 0.9286 | 0.0000 |
| plugin/MCP | 22 | 0.6379 | 0.2439 | 0.5455 | 0.0000 |
| LSP | 26 | 0.7340 | 0.2526 | 0.6538 | 0.0000 |
| research/search | 34 | 0.7279 | 0.2632 | 0.6176 | 0.0000 |
| structured/data | 38 | 0.7654 | 0.2785 | 0.6579 | 0.0000 |

Aggregate sequence MRR was 0.2613 versus linear 0.7095, with sequence
Recall@1 at 0.0 on every slice. No contextual/generalization slice gained
≥0.02 MRR or Recall@1 over linear; every required slice regressed more
than 0.02 MRR versus linear; no-tool F1 (`None` versus best baseline
`1.0`) missed its tolerance. The quality, gain, no-tool, and
generalization requirements therefore failed on valid semantics.

Counterfactual pair accuracy: **0 of 13 pairs** (0.0). The ranker chose
the expected changed label on neither side of any pair.

## 5. Calibration evidence

Raw and threshold-calibrated values were both Brier `0.1785673481`, ECE
`0.5270484328`, and NLL `0.5478500724`. The calibrated no-tool decision
metrics and all per-case raw/calibrated probabilities are preserved in the
result. The calibration gate passed because calibration stayed within the
frozen tolerance; no post-holdout recalibration occurred. The frozen
dev-selected threshold was used as-is.

## 6. Retrieval, promotion, and authority evidence

- Corrected universe/K identity on every point: native (universe 3, K=16)
  recall `1.0000` over 170 relevant; 64-tool universe (K=16) recall
  `0.9294` over 170 relevant / 158 recovered against `0.98`; 128-tool
  universe (K=16) recall `0.9294` over 170 relevant / 158 recovered
  against `0.95`. Retrieval gate failed on measured evidence — every
  labeled relevant tool was present in the expanded fixtures (the
  historical survivor-bias defect is gone), so 0.9294 is genuine
  shortlist recall, not a lookup artifact.
- Promotion relevant-tool recall: `0.0000` against `0.50`.
- No-tool promotion rate: `0.0000` against `0.10`.
- Irrelevant-tool promotion rate: `0.0000` against `0.15`.
- Mean and p95 promoted tools: `0.0` and `0.0` (cap 2 respected).
- Mean and p95 schema bytes: `0` and `0` (budget 16 KiB respected).
- Authority violations: `0` retrieved, `0` promoted; authority gate passed.
- Promotion gate failed because useful relevant-tool promotion recall was
  absent. With valid v3 no-tool semantics, the zero promotion recall is
  substantive negative evidence per the plan, not a harness defect.

## 7. Release resource evidence

| Measure | Value | Frozen limit |
|---|---:|---:|
| Encoder weights | 90,868,376 B | 134,217,728 B |
| Ranking head | 3,384 B | report-only |
| Tokenizer | 231,508 B | report-only |
| Release binary | 79,684,224 B | report-only |
| Comparable binary | 75,802,912 B | report-only |
| Feature delta | 3,881,312 B | report-only |
| Peak RSS | 287,522,816 B | report-only |
| Process-cold first/median/p95 | 411 / 388 / 411 ms | 10,000 ms p95 |
| Warmed rank p50/p95/max | 64,713 / 75,154 / 83,102 us | report-only |
| Retrieval p50/p95/max | 18,946 / 37,423 / 51,198 us | report-only |
| Total qualification | 38,959 ms | 600,000 ms |
| Encoder forwards/case | 2.0 | report-only |
| Cache warm/cold count | 47 / 1 | report-only |

The resource gate passed.

## 8. Gate-by-gate disposition

| Gate | Outcome |
|---|---|
| zero-leakage-v3 | pass |
| aggregate-mrr-vs-linear-v3 | fail |
| contextual-slice-gain-v3 | fail |
| no-tool-f1-v3 | fail |
| slice-generalization-v3 | fail |
| calibration-v3 | pass |
| retrieval-v3 | fail (measured 0.9294/0.9294) |
| authority-v3 | pass |
| promotion-v3 | fail |
| resource-v3 | pass |
| Overall | **D — no useful quality gain** |

The result is not A (quality and retrieval fail), not B (quality fails),
not C (retrieval at 0.9294 does not independently qualify), and not E
(all protocol, leakage, fixture, retrieval-identity, and authority
correctness checks passed).

## 9. Verification executed

| Command | Result |
|---|---|
| Commit A hosted CI (`CI`, run `35697246685`) | success before final evaluation |
| `cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_qualification` | 20 passed |
| `cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::` | 102 passed |
| `cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- --test-threads=1` | 4,939 passed |
| `cargo test --locked -p codegg --lib -- --test-threads=1` | 4,871 passed |
| `cargo test --locked -p codegg --tests -- --test-threads=1` | 8,749 passed; 1 ignored |
| `cargo test --locked --features tool-advisor-encoder-training -- --test-threads=1` | 8,816 passed; 3 ignored |
| member crates (core, config, protocol, git, egggit, eggsentry, eggcontext, providers, egglsp) | 2,923 passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `scripts/verify.sh quick` | pass |
| `cargo fmt --all -- --check` | pass |
| `git diff --check` | pass |
| release feature build | pass |
| final qualification command | pass; result written once |

Chunked sweeps ran on the C001 tree; Commit A added only the
preregistration JSON and one test-only regression test, and all focused,
feature, clippy, fmt, and quick gates were re-run green on the Commit A
tree that produced the release binary and the result.

## 10. Downstream unblock audit

- C001 is closed and its hard dependency was satisfied before Commit A.
- C002 is now closed with disposition D.
- Existing live-primary-model M004 remains blocked: only disposition A
  could make it dependency-ready, and the original operator, provider,
  trajectory, and resource prerequisites also remain unsatisfied.
- No other registered implementation plan lists C002 as a hard dependency
  or can be made ready by this D result.
- No new corrective pass is registered. A future positive architecture or
  model experiment would require a separately approved plan and fresh
  preregistration; this closure does not reopen historical M005, v2, or C001.

## 11. Roadmap and registry disposition

C002 is closed as a qualification process with a negative model
disposition. The qualification-evidence corrective addendum is closed: its
exit conditions (corrected universe/K identity, semantic validators, zero
leakage, frozen model, one preregistered A/B/C/D/E run) are all satisfied.
The result artifact is the authoritative final evidence; historical M005,
v2, and C001 closures remain immutable. Live M004 remains explicitly
blocked.
