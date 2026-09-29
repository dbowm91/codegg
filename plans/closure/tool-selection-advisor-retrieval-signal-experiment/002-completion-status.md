# Tool-Selection Advisor Retrieval-Signal Experiment M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-deterministic-retrieval-signal-v2.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m002--deterministic-retrieval-signal-v2`

Repository baseline reviewed: `ca44cdea`

Implementation commits:

- `6baac9ec` — add M002 deterministic retrieval signal sweep harness
- `1636e5b` — validate M002 current-step projected inputs and full-universe fingerprints (M010)
- `ca44cdea` — preserve the complete negative M002 frontier receipt and begin closure

## 1. Executive finding

M002 closes with a valid negative result. The frozen deterministic Retrieval Signal V2 sweep completed all 90 preregistered arm/universe/K frontier points across 62 dev cases and the 64/128/256-candidate universes. No point met all three frozen recall gates at K<=32. The maximum 256-universe result was 64/69 against the required 66; zero authority violations occurred. The evidence supports the preregistered negative-but-valid path: deterministic signal alone is insufficient, and the conditional frozen-encoder projection experiment M003 may proceed.

This is an experimental infrastructure result; no production catalog, routing, advisor activation, or model-selection behavior changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reproduce the controlling frozen preregistration | Receipt names protocol `retrieval-signal-m002-frontier-v1`; preregistration SHA-256 `4e90a05434fc839ba5f0191b590bf80c2bf796f0b0b9aed21b0a939ceff36e` | Pass | Matches the M009-corrected controlling receipt fingerprint. |
| Use M006 current-step labels and frozen dev split | Receipt: M006 decision SHA-256 `24e3783f1cc1c3d8934bac1592607702966112554db8ca1215087f90cfd09756`; dataset SHA-256 `06da7e530799df915c12ceaefe1076e40b70047c6f4276f37d70685e2c2d3582`; dev partition SHA-256 `b804b7d8c3ea981d2f53e32d37357aa39501159f8fc37bfded64c8fc38dc52a9` | Pass | 69 eligible relevant rows in each universe; required hits are 69/68/66. |
| Evaluate every preregistered point and preserve authority | `assets/tool-advisor/retrieval-signal-m002-frontier.json`; 10 arms × 3 universes × K16/24/32 = 90 points | Pass | 62 cases; zero authority violations. |
| Decide the frozen retrieval gates without post-result tuning | Complete receipt and per-tool frontier | Negative, valid | No point clears 64>=69, 128>=68, and 256>=66 simultaneously. Best observed hits: U64 65/69; U128 65/69; U256 64/69. |
| Record semantic encoder identity and costs | Receipt encoder manifest SHA-256 `e671e6876111ff9e0be190695a92332880c8bc352a30ee9542da4d4916a9bf3d`; load 15,708.36ms; warm query p50 214.71ms; warm descriptor p50 194.53ms | Pass | Five warm repeats; values are experiment measurements, not production latency claims. |
| Preserve the receipt exactly | Committed asset SHA-256 `7045d4fccb900c4b0f17dfb7f28e92d2c60d05385b5892d2cffe7145d7dd0a05` | Pass | Matches the independently generated ignored copy under `target/tool-advisor/retrieval-signal-m002/frontier.json`. |
| Run advisor tests and repository sanity checks | Feature-gated advisor suite; `scripts/verify.sh quick` | Pass | Suite: 205 passed, 0 failed, 7 ignored. Quick verification result recorded below. |

Persistent misses remain bounded and visible in the receipt, including `glob`, `lsp_rename`, `table_filter`, and `write`; no new lexical fields, synonyms, or post-result tuning were introduced.

## 3. Production implementation evidence

The M002 implementation is isolated in the experiment-owned `src/tool_advisor/retrieval_signal_m002.rs` path and its tests. It implements frozen query/descriptor representations, deterministic lexical and semantic arms, descriptor caching, candidate-universe expansion, authority-filtered frontier evaluation, and machine-readable receipt generation. M010's preflight corrections ensure M006's exact current-step exclusions are applied consistently to relevance and preferred order, validate projected cases, and fingerprint the full 64/128/256 universes before loading the encoder.

M002 adds no production selection behavior and does not modify the historical corpus, split assignments, M001/M002/M006 preregistrations, catalog semantics, or selected ranker artifact.

## 4. Verification executed

### Commands run

```bash
rustup run 1.98.1 cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_signal_m002::tests::m002_preregistered_dev_frontier_sweep -- --ignored --nocapture
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig rustup run 1.98.1 cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
scripts/verify.sh quick
cargo clippy --workspace --all-targets --features server,plugins,lsp-test-support -- -D warnings
git diff --check
```

The completed ignored sweep ran once and produced the identical committed and `target/` receipt copies. Its original interactive session was lost on a server restart; the still-running process was found and allowed to finish, and the complete receipt was recovered and hash-compared. No second sweep was started.

### Results

- Full frozen M002 frontier sweep: completed; valid negative result; 90 points, zero authority violations. The durable receipt is the evidence of completion and matches its independent target copy byte-for-byte. The interrupted terminal session did not preserve a shell exit code.
- Feature-gated advisor tests: passed, 205 passed / 0 failed / 7 ignored / 4,877 filtered out. The first local attempt failed at link time because `pkg-config` selected arm64 MacPorts `liblzma` for this x86_64 host. Rerunning with `PKG_CONFIG_PATH=/usr/local/lib/pkgconfig` selected x86_64 Homebrew `liblzma` and passed. This is an environment-only link-path correction; no source or dependency change was made.
- `scripts/verify.sh quick`: passed, including rustfmt, agent schema, architecture/authority guards, and locked workspace check.
- `cargo clippy --workspace --all-targets --features server,plugins,lsp-test-support -- -D warnings`: passed. This uses the repository's supported full feature set without enabling real-server tests.
- `cargo fmt --all -- --check`: included in `scripts/verify.sh quick`.
- `git diff --check`: pass on the final closure commit contents.

## 5. Invariant review

- **Candidate authority:** all rankings remain within the expanded deferred-candidate universe; receipt records zero authority violations.
- **Frozen evaluation:** only the preregistered dev partition, M006 current-step relevance decision, arms, universes, K values, and gates were used.
- **No train/test leakage:** M002 does not train parameters and uses no test, v2, v3, or future-v4 data for selection.
- **No historical mutation:** source corpus and historical receipts remain unchanged; the derived exclusions apply only to M002's current-step view.
- **No production behavior change:** M002 remains experimental infrastructure and does not activate or alter the production advisor.

## 6. Failure and recovery review

M010 moved projected-case validation and the complete universe fingerprint preflight ahead of encoder loading. The prior pre-corrective attempt produced no frontier receipt and is not treated as evidence. During this run the tool session was interrupted, but the underlying process continued; the completed output was accepted only after recovering the full JSON receipt and matching its SHA-256 against the ignored target copy. A later feature-test link error was resolved by selecting the host-architecture `liblzma` via `PKG_CONFIG_PATH`, after which the entire non-ignored advisor suite passed. No partial metrics or duplicate full sweep were accepted.

## 7. Migration and compatibility review

No persisted schema, user configuration, wire protocol, or migration changed. Retrieval Signal V2 is versioned and experiment-local; production `ToolCatalog` behavior and the existing advisor path remain unchanged.

## 8. Security review

Candidate-authority boundaries are explicitly checked for every frontier point and have zero violations. Experiment inputs and receipts are local frozen assets. No secrets, runtime defaults, or unauthorized candidate surface were added.

## 9. Documentation and operations

- The roadmap and registry record the completed negative result and the dependency transition.
- The committed frontier JSON is the canonical machine-readable receipt.
- The required all-features Clippy command is intentionally omitted under the repository's explicit real-server-test restriction; the canonical quick verification is the workspace sanity check.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| None | No unresolved correctness or authority findings in M002 scope. | None. | None. |

## 11. Roadmap disposition

M002 closes negative-valid. Deterministic Signal V2 does not meet the frozen large-catalog recall gates; the conditional M003 frozen-encoder projection experiment may proceed. M004 remains blocked because neither M002 nor M003 produced a positive retrieval point. M005 remains blocked on a positive M004 operating point. No live-primary-model advisor work is unblocked.

## 12. Registry updates and dependency audit

- M002: closing → closed.
- M003: blocked/conditional → ready. M002's valid negative satisfies its result dependency, and M006's positive current-step inferability re-audit satisfies M003's M001 hard dependency. The historical blocked M001 closure remains immutable.
- M004 remains blocked pending a positive M002 or M003 retrieval result.
- M005 remains blocked pending a positive M004 operating point.
- The other registered blocked work audited in `plans/registry.md` remains blocked by its named independent upstream or operational evidence. No additional plan becomes dependency-ready from this closure.
- No corrective plan is required by M002 evidence.
