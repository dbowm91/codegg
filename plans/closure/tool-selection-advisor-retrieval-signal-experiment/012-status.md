# Tool-Selection Advisor Retrieval-Signal Experiment M012 — Closure Status

Status: closed (positive)

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/012-train-inferability-taxonomy-coverage-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m003--frozen-encoder-learned-retrieval-projection`

Repository baseline reviewed: `3223399a`

## 1. Executive finding

The M011 classifier gap is corrected with a frozen, generic, audit-only action-family table. The complete frozen train partition audit passes with no `other-evidence-defect` labels, and the exact current-step optimizer view is reproducibly frozen. The correction changes no retrieval tokens, scoring, corpus bytes, labels, partitions, M006 target, or M003 search grid.

M003's dependency audit is now positive: M006 supplies the positive current-step dev-label re-audit, M002 is closed with a valid negative result, and this M012/M011 train audit approves the optimizer labels. M003 is returned to ready. M004 and M005 remain blocked on positive retrieval/operating-point evidence.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Freeze generic audit-only taxonomy | `AUDIT_PARAPHRASE_GROUPS`, version 1; SHA-256 `919053a208e98776f8401248db8058b57b2011b4ab26efa29f86cb93b2001404` | Pass | Tool-agnostic groups include open/read; exact same-action terms classify query-explicit, cross-family variants query-paraphrase. |
| Audit frozen train partition only | Receipt `assets/tool-advisor/retrieval-signal-m003-train-audit.json` | Pass | 132 train cases; 102 cases with labels; exactly 102 optimizer cases. Frozen train partition SHA-256 `c67df3caf97f05b5b62e2599d933c72dc511132da4e9c98fc2d184de2f190fdf`. |
| Reject unsupported primary labels and partition drift | Focused audit tests and `audited_optimizer_cases` | Pass | Full train audit has no other-evidence-defect; non-train indexes are rejected. |
| Apply M006 current-step-only target | Receipt and M006 hash | Pass | M006 decision SHA-256 `24e3783f1cc1c3d8934bac1592607702966112554db8ca1215087f90cfd09756`; 19 implicit-secondary labels excluded. |
| Preserve corpus and dev provenance | Receipt fingerprints | Pass | Corpus SHA-256 `06da7e530799df915c12ceaefe1076e40b70047c6f4276f37f70685e2c2d3582`; dev partition SHA-256 `b804b7d8c3ea981d2f53e32d37357aa39501159f8fc37bfded64c8fc38dc52a9`. |
| Freeze exact optimizer input | Receipt + `target/tool-advisor/retrieval-signal-m003-train-audit/optimizer-cases.jsonl` | Pass | Optimizer-input SHA-256 `b1aec285c2bcc93ebb0945385dbc6cc2b1bbe42f77010b7432dfd16bf5a1b0ba`. |
| Reproduce committed receipt | Two focused test runs | Pass | Receipt bytes remained SHA-256 `b761dba49d6ec6324b28a6cc8a4ec36af06b399fabe8438dfab0457c86da3193` before and after rerun. |

Label counts across 138 positive train labels: `query-explicit` 113, `query-paraphrase` 4, `descriptor-incomplete` 2, `implicit-secondary` 19. The first three classes' 119 labels are included; all 19 implicit-secondary labels are excluded. Grade counts are 2:72 and 3:66.

## 3. Production implementation evidence

`src/tool_advisor/retrieval_signal_m003_audit.rs` applies the frozen direct/schema checks first, then a separately versioned generic paraphrase table, then sibling-aware current-step adjudication. The table is independent of retrieval tokenization and contains no candidate-specific aliases. Exact generic action matches, including short `find`, retain exact query substring support and classify as query-explicit; equivalent generic-family variants classify as query-paraphrase. The table fingerprint is asserted by test and recorded in the receipt.

The receipt is committed at `assets/tool-advisor/retrieval-signal-m003-train-audit.json`. The exact optimizer cases are emitted under `target/` and are not committed. Output remains fail-closed and is atomically replaced only after all frozen fingerprints and labels pass.

## 4. Verification executed

```text
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig rustup run 1.98.1 cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_signal_m003_audit
scripts/verify.sh quick
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig rustup run 1.98.1 cargo clippy --workspace --all-targets --features server,plugins,lsp-test-support -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Results: focused tests 4 passed / 0 failed; quick verification passed; supported workspace Clippy passed; formatting and whitespace checks passed. The repository's `--all-features` posture is intentionally not used because it pulls real-server tests; the supported feature set above matches repository guidance.

## 5. Invariant review

- Frozen corpus, labels, partitions, M006 decision, retrieval construction, scoring, and M003 grid remain unchanged.
- No dev/test rows enter optimizer cases; optimizer indexes must exactly match the frozen train partition.
- No model was trained and no dev/test outcome was used to select taxonomy entries.
- The audit-only generic table does not affect production retrieval behavior.
- All retained labels have allowed query/schema support; implicit-secondary labels are omitted per M006.

## 6. Security and compatibility review

The pass is deterministic and offline over repository-owned synthetic examples and bounded built-in schemas. It adds no runtime retrieval path, persistence/schema migration, network, subprocess, credential, or execution surface.

## 7. Unresolved findings

None for M012. M002's valid negative result remains unchanged; no eligible deterministic arm cleared recall gates. That result does not satisfy M004's positive-retrieval dependency.

## 8. Roadmap disposition and dependency audit

- M012: active → closed (positive); classifier corrective, reproducible receipt, tests, quick, and supported Clippy are complete.
- M003: blocked → ready. Its hard dependencies are satisfied by M006's positive current-step dev-label audit, M002's valid negative closure (which authorizes the preregistered learned projection experiment), and the positive frozen train-label receipt produced here.
- M004 remains blocked pending positive M002 or M003 retrieval gates.
- M005 remains blocked pending M004.
- Registry scan found no other plan newly unblocked by this closure.

The historical M011 blocked closure is retained unchanged as evidence of the defect. This closure owns the corrective evidence and does not rewrite that history.
