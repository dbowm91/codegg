# Tool-Selection Advisor Retrieval-Signal Experiment M012 — Train Inferability Taxonomy Coverage Corrective

Status: active

Repository baseline: `4bb5c197`

Source roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m003--frozen-encoder-learned-retrieval-projection`

Long-term requirements: `plans/000-long-term-specification.md`; `plans/002-long-term-roadmap.md`

Applicable ADRs: `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: evidence/infrastructure

## 1. Objective

Correct the generic M001 audit-classifier coverage gap discovered by M011, then rerun the frozen train-partition audit and emit an optimizer-input receipt only if every retained primary label is supported and no evidence defect remains.

## 2. Why this milestone is ready

M011 closed blocked with a deterministic finding: its frozen classifier labels `read` for the current-step query “Open dep_lock” as `other-evidence-defect` because the audit-only generic verb table omits the ordinary `open`/`read` paraphrase. This is a taxonomy implementation gap, not a missing query cue or an M002 retrieval outcome. M001's taxonomy explicitly permits `query-paraphrase`; an audit-only, tool-agnostic correction can repair classification without changing retrieval text, M003's frozen model grid, or the M006 relevance decision.

## 3. Current implementation evidence

- Blocked M011 finding: `plans/closure/tool-selection-advisor-retrieval-signal-experiment/011-status.md`.
- Frozen audit classifier and paraphrase table: `src/tool_advisor/retrieval_signal.rs`.
- M011 train-only receipt builder and tests: `src/tool_advisor/retrieval_signal_m003_audit.rs`.
- Frozen M006 relevance target: `assets/tool-advisor/retrieval-signal-m006-decision.json`.

## 4. Invariants that must not regress

- This change applies only to inferability adjudication; it must not alter M002 retrieval text, scoring, M003 optimizer architecture/grid, recall gates, or M006 target.
- Add only generic, tool-agnostic operation paraphrases with documented semantic equivalence and exact supporting text; no candidate-name aliases or per-tool rules.
- Freeze the complete generic audit table and its hash before the full train re-audit; do not select mappings from dev/test outcomes.
- Keep train/dev/test partitions and corpus labels byte-identical.
- M011 must fail closed on unsupported primary labels, empty support, or partition/fingerprint drift.

## 5. Scope

In:

- add an audit-only paraphrase table separate from Signal V2 retrieval features;
- cover the generic action vocabulary needed by M001's query-paraphrase class, including open/read, with deterministic matching and rationale;
- classify the full train partition and record every label class/support before filtering current-step implicit-secondary pairs;
- emit M011's frozen receipt and exact optimizer cases only when the full audit is positive;
- add regressions for `Open dep_lock` → `read`, exact support text, generic matching across non-tool-specific contexts, and rejection of dev/test indexes.

Out:

- editing the frozen corpus, labels, M006 receipt, M001 historical closure, dev metrics, retrieval scoring, synonyms used by a retriever, projection grid, or thresholds;
- training or evaluating any projection/model.

## 6. Required production changes

Implement the audit-only taxonomy correction in M011's audit module (or a clearly named shared helper), with a versioned constant and fingerprint recorded in the receipt. Do not change descriptor/query construction or lexical/semantic retrieval arms. The existing M011 receipt path is the only committed output path.

## 7. Ordered work packages

1. Freeze a generic audit-only verb/paraphrase table and its canonical hash in code and tests.
2. Apply the existing M001 direct/schema checks first, then the frozen generic paraphrase check, then M001 sibling-aware secondary adjudication.
3. Audit every positive label in the deterministic train partition, accumulating the complete diagnostic set; fail before output on any `other-evidence-defect` or missing support.
4. Apply M006 `current-step-only`: exclude `implicit-secondary`, retain supported grades, and omit no-tool/empty-positive cases as preregistered.
5. Generate the content-addressed receipt and exact optimizer-input file; verify repeat runs are byte-identical and source corpus bytes do not change.

## 8. Failure, cancellation, restart, contention semantics

Offline deterministic calculation only. Any mismatch or evidence defect fails before replacing a prior accepted receipt. Writes use a temporary sibling and atomic rename after all checks, so partial outputs are never accepted. There is no model state or concurrency surface.

## 9. Compatibility and migration

No runtime catalog, persisted user data, schema, or protocol change. The classifier extension is audit-only and must not become a production retrieval synonym source.

## 10. Required tests

- generic open/read paraphrase is classified query-paraphrase with exact supporting query substring;
- audit paraphrase matching is independent of candidate/tool identity;
- all train labels classify to explicit classes and unsupported primary labels remain hard errors;
- no dev/test indexes enter optimizer cases;
- receipt and optimizer input are deterministic, fingerprint-pinned, and corpus-immutable;
- receipt is not emitted/replaced on any failed audit.

## 11. Required verification commands

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_signal_m003_audit
scripts/verify.sh quick
cargo clippy --workspace --all-targets --features server,plugins,lsp-test-support -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 12. Documentation updates

Update the M011 closure with the corrective result, M003's handoff dependency, the retrieval-signal roadmap, and the registry.

## 13. Acceptance criteria

The complete frozen train audit reports no `other-evidence-defect`, every retained positive includes allowed query support, all implicit-secondary labels are excluded, and the receipt/optimizer fingerprint reproduces exactly. A positive completion makes M003 ready. Any remaining defect leaves M003 blocked without training.

## 14. Stop conditions

Stop if a proposed paraphrase is tool-specific, cannot be justified as generic action equivalence, requires dev/test/model outcomes, or changes retrieval features. Stop before output on any remaining unsupported primary label or fingerprint drift. Do not relabel corpus data or broaden M003's search grid.

## 15. Closure evidence required

Record the frozen audit-table hash, corpus/train/dev/optimizer fingerprints, label counts by class and grade, exact verification results, deterministic receipt comparison, and dependency audit.

## 16. Handoff notes

The known `open`/`read` pair comes from M011's stop record. Keep the correction audit-only, freeze the generic table before processing the train partition, and inspect no dev/test outcome while doing so.
