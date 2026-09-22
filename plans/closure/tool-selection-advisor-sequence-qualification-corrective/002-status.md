# Tool-Selection Advisor Sequence Qualification Corrective C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-qualification-corrective/002-preregistered-release-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-qualification-corrective-addendum.md#c002--preregistered-release-mode-requalification`

Repository baseline reviewed: `df6693da`

Implementation and evidence commits:

- `621032e` — advisor: implement sequence qualification v2 harness
- `8a59d45` — plans: close sequence qualification corrective c001
- `9a9636f` — plans: freeze c002 sequence qualification protocol
- `a23eadd` — plans: correct c002 preregistration canonical hash
- `df6693d` — plans: correct c002 holdout fingerprint

Qualification result:

- `assets/tool-advisor/sequence-qualification-v2-result.json`
- `df6693da` — final preregistration commit SHA echoed by the result
- disposition: **D — no useful quality gain**

## 1. Executive finding

C002 closes with a valid negative release-mode qualification. The selected
MiniLM packed-marker artifact was evaluated once on the fresh C001 holdout
after protocol, artifact, partition, manifest, leakage, and resource checks
passed. The holdout was not used for tuning. The sequence ranker did not meet
the preregistered quality, generalization, retrieval, or promotion gates, so it
does not qualify for live M004. This is an honest D disposition, not a
correctness or harness failure.

The two earlier attempts stopped before producing a result artifact: the first
found a protocol canonicalization mismatch and the second found a typed-loader
holdout fingerprint mismatch. They changed no model, holdout, gates, or result
inputs. The canonical Rust protocol hash and loader-derived holdout fingerprint
were corrected in pushed revisions, with regression tests, before the one valid
qualification run. No result existed until the final run recorded here.

## 2. Preregistration and run receipt

| Item | Frozen evidence | Result |
|---|---|---|
| Protocol | `c002-preregistered-release-sequence-qualification-v2` | pass |
| Preregistration commit | `df6693daadb1ca0bb83c1e527bc77a70364bbc56` | echoed in result |
| Protocol hash | `2fc132c1c180a56b0dc7b1aca6533ca3744e3b6f2598ebc0f6ccec7340845e2b` | pass |
| Historical dataset | `06da7e530799df915c12ceaefe1076e40b70047c6f4276f37d70685e2c2d3582` | pass |
| Fresh holdout | `9da9c6abae05b121e6af159b75d10980ec328d4ca300c6b8308b59f6f182b0b7` | pass |
| Fresh manifest | `63bac276394b1d6548829f514119f1b70214e80112f1e205c3120f5236a610ba` | pass |
| Selected sequence artifact | `01b5c368b4e762dbe6ca284694b290b72e17ec64606e1f26a21510721f9a10e8` | pass |
| Target | Apple Silicon macOS, CPU device; filesystem cache uncontrolled | reported |
| Final command | `./target/release/codegg tool-advisor sequence-encoder-qualify-v2 --prereg assets/tool-advisor/sequence-qualification-v2-preregistration.json --prereg-commit df6693da --json` | executed once |

Hosted CI for `df6693da` was available as GitHub Actions run
`35685174158` (`https://github.com/dbowm91/codegg/actions/runs/35685174158`).
The local closure evidence also includes the passing `scripts/verify.sh quick`,
focused feature tests, workspace tests, and all-feature Clippy. The hosted run
was still in progress while this closure was prepared; its conclusion remains
an operational follow-up to record if it changes. It does not affect the
already-completed release qualification result.

## 3. Holdout integrity and leakage evidence

| Measure | Value | Gate |
|---|---:|---|
| Cases | 128 | pass |
| Leakage groups | 128 | pass |
| Exact overlap | 0 | pass |
| Normalized overlap | 0 | pass |
| Template overlap | 0 | pass |
| Explicit family overlap | 0 | pass |

Required slice floors were met: hard-negative 32, counterfactual 16,
unknown-renamed 26, no-tool 16, multi-tool 16, context-v2-long-session 22,
plugin/MCP 20, LSP 20, research/search 40, and structured/data 32.

## 4. Quality and slice evidence

The complete metric payload is in the machine-readable result. The following
table records every declared slice's cases, frozen-linear MRR/Recall@1, and
sequence MRR/Recall@1.

| Slice | Cases | Linear MRR | Sequence MRR | Linear R@1 | Sequence R@1 |
|---|---:|---:|---:|---:|---:|
| aggregate | 128 | 0.4805 | 0.2917 | 0.1250 | 0.0000 |
| hard-negative | 32 | 0.2656 | 0.1667 | 0.0625 | 0.0000 |
| counterfactual | 16 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| unknown-renamed | 26 | 0.4615 | 0.2821 | 0.1154 | 0.0000 |
| no-tool | 16 | 0.0000 | 0.0000 | 0.0000 | 0.0000 |
| multi-tool | 16 | 1.0000 | 0.3333 | 1.0000 | 0.0000 |
| context-v2-long-session | 22 | 0.4091 | 0.2424 | 0.1364 | 0.0000 |
| plugin/MCP | 20 | 0.4750 | 0.2833 | 0.1000 | 0.0000 |
| LSP | 20 | 0.5250 | 0.3000 | 0.1500 | 0.0000 |
| research/search | 40 | 0.4375 | 0.2917 | 0.1250 | 0.0000 |
| structured/data | 32 | 0.5000 | 0.2917 | 0.1250 | 0.0000 |

Aggregate sequence MRR was 0.2917 versus linear 0.4805. The contextual-slice
gain and slice-generalization requirements therefore failed, as did the
no-tool F1 requirement (`sequence: None`, best baseline `1.0`).

## 5. Calibration evidence

Raw and threshold-calibrated values were both Brier `0.2174509943`, ECE
`0.6560833864`, and NLL `0.6278141789`. The calibrated no-tool decision
metrics and all per-case raw/calibrated probabilities are preserved in the
result. The calibration gate passed because calibration stayed within the
frozen tolerance; no post-holdout recalibration occurred.

## 6. Retrieval, promotion, and authority evidence

- Retrieval recall at 64 tools: `0.0000` against `0.98`; at 128 tools:
  `0.0000` against `0.95`; retrieval gate failed.
- Promotion relevant-tool recall: `0.0000` against `0.50`.
- No-tool promotion rate: `0.0000` against `0.10`.
- Irrelevant-tool promotion rate: `0.0000` against `0.15`.
- Mean and p95 promoted tools: `0.0` and `0.0`.
- Mean and p95 schema bytes: `0` and `0`.
- Authority violations: `0`.
- Promotion gate: failed because useful relevant-tool promotion recall was
  absent, despite the safety/authority counts remaining zero.

## 7. Release resource evidence

| Measure | Value | Frozen limit |
|---|---:|---:|
| Encoder weights | 90,868,376 B | 134,217,728 B |
| Ranking head | 3,384 B | report-only |
| Tokenizer | 231,508 B | report-only |
| Release binary | 79,568,288 B | report-only |
| Comparable binary | 75,802,912 B | report-only |
| Feature delta | 3,765,376 B | report-only |
| Peak RSS | 280,641,536 B | report-only |
| Process-cold first/median/p95 | 340 / 345 / 352 ms | 10,000 ms p95 |
| Warmed rank p50/p95/max | 51,683 / 55,891 / 62,861 us | report-only |
| Retrieval p50/p95/max | 14,073 / 32,628 / 35,626 us | report-only |
| Total qualification | 21,462 ms | 600,000 ms |
| Encoder forwards/case | 2.0 | report-only |
| Cache warm/cold count | 13 / 1 | report-only |

The resource gate passed.

## 8. Gate-by-gate disposition

| Gate | Outcome |
|---|---|
| zero-leakage-v2 | pass |
| aggregate-mrr-vs-linear-v2 | fail |
| contextual-slice-gain-v2 | fail |
| no-tool-f1-v2 | fail |
| slice-generalization-v2 | fail |
| calibration-v2 | pass |
| retrieval-v2 | fail |
| promotion-v2 | fail |
| resource-v2 | pass |
| Overall | **D — no useful quality gain** |

The result is not A, so the live-primary-model M004 plan remains blocked. It is
not B, C, or E: the resource gate passed, retrieval did not provide useful
recall evidence, and all protocol/leakage/authority/harness correctness gates
required for an eligible negative result passed.

## 9. Verification executed

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo check --locked --features tool-advisor-encoder-training` | pass |
| `cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_qualification` | 8 passed |
| `cargo test --workspace --locked -- --test-threads=1` | 11,672 passed; 3 ignored |
| `cargo test --locked --features tool-advisor-encoder-training -- --test-threads=1` | 8,797 passed; 3 ignored |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `scripts/verify.sh quick` | pass |
| `git diff --check` | pass |
| release feature build | pass |
| final qualification command | pass; result written |

## 10. Downstream unblock audit

- C001 is closed and its hard dependency is satisfied.
- C002 is now closed with disposition D.
- Existing live-primary-model M004 remains blocked: only disposition A could
  make it dependency-ready, and the original operator, provider, trajectory,
  and resource prerequisites also remain unsatisfied.
- No other registered implementation plan lists C002 as a hard dependency or
  can be made ready by this D result.
- No new corrective pass is registered. A future positive architecture or
  model experiment would require a separately approved plan and fresh
  preregistration; this closure does not reopen historical M005.

## 11. Roadmap and registry disposition

C002 is closed positively as a qualification process with a negative model
disposition. The corrective addendum is closed, the result artifact is the
authoritative final evidence, and the historical M005 closure remains
immutable. Live M004 remains explicitly blocked.
