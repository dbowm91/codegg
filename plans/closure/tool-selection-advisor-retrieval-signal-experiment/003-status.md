# Tool-Selection Advisor Retrieval-Signal Experiment M003 — Closure Status

Status: closed (negative-valid)

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/003-frozen-encoder-retrieval-projection.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m003--frozen-encoder-learned-retrieval-projection`

Repository baseline: `36e09f69` (frozen projection contract and hard-negative identities)

## 1. Executive finding

The preregistered frozen-encoder projection experiment completed all 81 grid points on the frozen 62-case dev partition. No point cleared the retrieval recall gates at K≤32, so no projection was selected and no model artifact was emitted. This is a valid negative result under the plan's stop condition. M004 and M005 remain blocked.

The strongest Recall@32 was 66/69 (0.95652) at U64, 64/69 (0.92754) at U128, and 63/69 (0.91304) at U256. The frozen gates were respectively 69/69 (≥0.99), 68/69 (≥0.98), and 66/69 (≥0.95). Maximum authority violations across the grid were zero. No arm passed the generalization guards.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Freeze realization choices and hard negatives before fitting | `36e09f69`; projection contract SHA-256 `5f098a77aa67a74ed476e944ad8a2de2004dbcb85dc9f1ec37730eebe70b3016`; hard-negative receipt `assets/tool-advisor/retrieval-signal-m003-hard-negatives.json` SHA-256 `17e6c1b2f3db1517a33d9e2df2770dd60ec60833cca5e2598e4c372834d7ed92` | Pass | 119 retained positive pairs, seven distinct frozen negatives per pair. |
| Use only approved train optimizer cases | M012 train audit and M003 receipt | Pass | 102 train optimizer cases; optimizer-input SHA-256 `b1aec285c2bcc93ebb0945385dbc6cc2b1bbe42f77010b7432dfd16bf5a1b0ba`. No dev/test/v2/v3 examples entered optimization. |
| Preserve data and encoder provenance | M003 receipt `assets/tool-advisor/retrieval-signal-m003-projection.json` | Pass | Dataset, train/dev partitions, encoder manifest/weights/vocabulary, representation schema, M001 preregistration, M002 receipt, and M006 decision hashes are recorded in the receipt. |
| Run the complete frozen grid and reproduce its receipt | Focused ignored test `m003_full_projection_sweep_emits_receipt` | Pass | 81 points; receipt SHA-256 `a8d3dbea771eb16c825fb5542bf3fc78d3581e53aeedae1a0a47406d80f65bf6`. |
| Clear recall and authority gates | Receipt | Negative-valid | Best recalls remain below all three frozen gates; all arms recorded zero authority violations. |
| Report resource evidence | Receipt and test process observation | Partial | Query projection p50 maximum was 0.358208 ms; descriptor projection/cache build maximum was 6535.007 ms. `incremental_rss_bytes` is null because the sweep did not capture a valid incremental measurement. Total process RSS observations are not a substitute for incremental RSS. No artifact was selected, so there is no projection artifact size. |
| Keep v3 diagnostic-only | Experiment execution record | Pass | No v3 evaluation was opened after the failed dev gates; no post-result search or tuning followed. |

The receipt reports baseline unknown/renamed MRR 0.67163 and name-masked description-only lexical Recall@32 0.71014. No grid point met all selection and generalization conditions. The detailed per-arm and slice metrics remain in the committed receipt.

## 3. Implementation and verification evidence

Implementation/frozen-contract commit: `36e09f69`.

Full sweep command:

```text
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig rustup run 1.98.1 cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_signal_m003::tests::m003_full_projection_sweep_emits_receipt --ignored --nocapture
```

Result: 1 passed; complete receipt emitted and internally checked for 81 grid points, disposition, and selection consistency. The hard-negative freeze test also passed before the sweep. Focused M003 unit tests passed (2 passed, 1 ignored).

Broad closeout verification:

```text
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
scripts/verify.sh quick
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig rustup run 1.98.1 cargo clippy --workspace --all-targets --features server,plugins,lsp-test-support -- -D warnings
cargo fmt --all -- --check
git diff --check
```

These closeout checks are recorded after execution in the implementation commit; the supported feature set follows repository guidance and excludes `--all-features` because that includes real-server tests.

Results: advisor suite 211 passed / 0 failed / 9 ignored; `scripts/verify.sh quick` passed; supported workspace Clippy passed; formatting and whitespace checks passed.

## 4. Invariant and compatibility review

- Frozen MiniLM encoder weights, tokenizer, corpus, partitions, M001 grid, M002 implementation/result, M006 target decision, and M012 optimizer audit are unchanged.
- Projection training consumed only the frozen M012 optimizer view and frozen train negatives.
- Retrieval remained advisory and authority-bounded; the receipt records zero authority violations for all arms.
- No runtime default, production retrieval path, persistence, network, or model-download behavior changed.
- No weights were selected, and no v3 diagnostic or v4 qualification was run.

## 5. Unresolved findings

The incremental RSS measurement required by the resource contract was not captured (`incremental_rss_bytes: null`). This limits resource-cost characterization but does not invalidate the negative result: selection was impossible because every arm failed the primary dev recall gates, and no artifact is eligible for deployment. The missing measurement should be captured if a future plan proposes a new projection sweep or deployment candidate.

No other unresolved correctness or authority findings were identified.

## 6. Roadmap disposition and dependency audit

- M003: active → implemented; complete negative-valid closure recorded here.
- M004 remains blocked because neither deterministic M002 nor learned M003 produced a positive retrieval operating point.
- M005 remains blocked on M004.
- M001 and earlier corrective closures remain immutable historical evidence.
- Registry dependency audit found no newly unblocked downstream plan. The retrieval-signal workstream has no eligible successor; any renewed retrieval-model search requires a new plan with a changed, justified architecture hypothesis.
