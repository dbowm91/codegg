# Tool-Selection Advisor Retrieval-Signal Experiment M001 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001-signal-sufficiency-audit-and-preregistration.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m001--signal-sufficiency-audit-and-preregistration`

Repository baseline reviewed: `88ebbb9685bc0af497bd5b2caac0819b9542d106`

Implementation commits or pull requests:

- `88ebbb96` — implement retrieval-signal M001 signal-sufficiency audit and preregistration (new `src/tool_advisor/retrieval_signal.rs`, `pub mod retrieval_signal` in `src/tool_advisor/mod.rs`)
- (this closure commit) — `assets/tool-advisor/retrieval-signal-m001-preregistration.json` receipt, `006-retrieval-evaluation-corrective.md` registration, registry/roadmap/plan status updates

## 1. Executive finding

M001 executed its audit and preregistration duties completely, then hit the
plan's own §4 hard stop: three of the four persistent-miss occurrences carry
relevance labels that are **not inferable from allowed `AdvisorContextV2`
state**. Each is a grade-2 supporting-workflow label whose query is fully
explained by a co-relevant sibling tool, with zero descriptor, schema, or
paraphrase cue for the labeled tool. Each occurrence is 1/72 of the dev
frontier, and the frozen u256 gate needs 69/72 while the best fused system
recovers 68/72 — so every one of them is gate-critical and material. Per the
plan, M001 therefore closes **blocked** (not positive): no gate is altered in
this workstream, no projection is trained to memorize hidden labels, M002
stays blocked, and a separate retrieval-evaluation corrective
(`006-retrieval-evaluation-corrective.md`) is registered to decide the
intended relevance target.

The fourth occurrence (`glob`, grade 3, sole label) is inferable
(`query-paraphrase`) and yields a precise, attributed signal gap: the v1
BM25 query shares **zero** tokens with the `glob` descriptor while its only
content word (`fixture`) collides with the expanded-filler template
vocabulary, burying the tool at BM25 rank 256 with score 0.0. That gap is
exactly what deterministic Signal V2 (identifier/capability terms, field
weighting, schema cues) is designed to test — once the evaluation target is
settled.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| §2 miss audit, all four tools, full occurrence fields | `run_miss_audit()` in `src/tool_advisor/retrieval_signal.rs`; receipt `miss_audit.occurrences` (4 rows) | pass | Live BM25 ranks at expanded-u256 K=32; semantic/fused ranks quoted-frozen from M003 `fb11a74f` with explicit provenance; no v3 input |
| §2 aggregate reproduction (52/72 BM25) | `miss_audit_reproduces_four_persistent_misses` test; live `eligible={64,128,256: 72}`, `recovered={64,128,256: 52}` | pass | Matches `retrieval_architecture::M004_BM25_RECOVERED/ELIGIBLE/DEV_CASES` exactly |
| §3 inferability taxonomy, exactly one class + rationale + allowed-text support | `classify_inferability` + sibling-aware adjudication; `inferability_classification_always_carries_supporting_text` test | pass | 1× query-paraphrase, 3× implicit-secondary; every row carries rationale + query-substring support |
| §4 hard stop on evaluation defects | `m002_ready=false`; `stop_condition` string in receipt; this blocked closure + 006 registration | pass | Stop triggered, gate untouched |
| §5 Signal V2 representation contract frozen | `RetrievalQueryV2`, `RetrievalDescriptorV2`, `QUERY/DESCRIPTOR_FIELD_ORDER`, byte caps, `PREREGISTERED_SYNONYM_LEXICON_ENTRIES=0` | pass | No per-tool aliases; generic normalization only |
| §6 schema plumbing audit | `schema_plumbing_audit()` + receipt `schema_plumbing`; live schemas captured as frozen literals for `glob`/`write` | pass | Finding: `table_filter` has no live Tool impl; `lsp_rename` has no standalone live impl; experiment-local representation preferred, no durable artifact change |
| §7 conditional M003 grid frozen | `PROJECTION_FAMILIES/LR_GRID/MAX_EPOCHS/TEMPERATURE_GRID/SEEDS/HARD_NEGATIVES/BATCH_SIZE/MAX_PROJECTION_PARAMS` + receipt | pass | No expansion permitted after M002/v3 inspection (M002 not run) |
| §8 training-target contract | Graded positives + train-only hard negatives from frozen pre-M003 scorers recorded in preregistration constants | pass | Conditional only; no training run in M001 |
| §9 frozen dev gates unchanged | `GATE_RECALL_64/128/256` = 0.99/0.98/0.95, K<=32, zero violations; receipt `gates` | pass | No relaxation |
| §10 compact receipt committed | `assets/tool-advisor/retrieval-signal-m001-preregistration.json` (317 lines, no embeddings) | pass | Content-identical to generator output; fingerprints match frozen references |
| §11 regression tests (9 required) | 11 tests in `retrieval_signal::tests`, all passing | pass | Covers all 9 required behaviors plus cache-key hygiene |
| No model trained; no gate changed (§13) | Code inspection: no encoder use, no optimizer, no threshold edits | pass | Positive-closure requirements otherwise unmet only via the §4 stop |

## 3. Production implementation evidence

- New module `src/tool_advisor/retrieval_signal.rs` (~1760 lines incl.
  tests), registered as ungated `pub mod retrieval_signal` in
  `src/tool_advisor/mod.rs`. Ungated placement is deliberate: the module is
  dependency-free (no encoder weights, no subprocesses, no network), so the
  audit runs on every host, including ones where
  `tool-advisor-encoder-training` does not compile (see §4).
- Deterministic primitives: `split_identifier_tokens` (snake/kebab/punct/
  camel), `signal_tokens`, `extract_schema_fields` (sorted, capped, never
  emits example/default/const/enum payloads), `live_builtin_schema`
  (frozen `glob`/`write` literals with provenance comments),
  `RetrievalQueryV2::serialize` (frozen 5-field order, 2 KiB/field,
  8 KiB total), `RetrievalDescriptorV2::{flat_text,
  field_labelled_text}`, `RetrievalCacheKeyV2` (descriptor-only).
- Audit engine: `expand_universe_m001` (documented mirror of
  `operating_point::expand_universe`), `bm25_full_order` (live ranks via
  `baseline_prediction`), `mirrored_bm25_score` (attribution-only score
  mirror, labeled `mirrored-catalog-bm25-v1`), `lexical_overlap_terms`,
  `classify_inferability` + `query_explicit_support` sibling adjudication,
  `run_miss_audit`, `m001_preregistration`, `prereg_fingerprint`
  (rejects v3 selection inputs before hashing).
- No changes to runtime advisor behavior, catalog scoring, durable
  artifacts, thresholds, or promotion logic. No production behavior change
  by design (evidence/infrastructure class).

## 4. Verification executed

### Commands run

```bash
cargo test --locked -p codegg --lib -- tool_advisor::retrieval_signal
cargo test --locked -p codegg --lib -- tool_advisor
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

The plan's literal first command
(`cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor`)
could not run on this host: `candle-core v0.11.0` fails to compile under
rustc 1.89.0 on aarch64-darwin (`stdarch_neon_f16` unstable-library
errors, pre-existing dependency/toolchain interaction, unrelated to this
change). The module's ungated placement is the justified substitute: the
full M001 suite runs without the encoder feature.

### Results

- `tool_advisor::retrieval_signal`: 11 passed / 0 failed.
- `tool_advisor` (base features): 55 passed / 0 failed.
- `check_execution_ownership.py`: guard ok.
- `scripts/verify.sh quick`: passed (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, tui-authority, http-route-disposition,
  audit-coverage, scheduler-bypass, eggwork routing, workspace check).
- `cargo fmt --all -- --check` and `git diff --check`: clean.
- Live audit output: 62 dev cases, 72 eligible relevant per universe,
  BM25 recovered 52 per universe (exact M004 reproduction); 4 missed
  occurrences (one per persistent tool), all BM25 rank 256 score 0.0 at
  expanded-u256.

## 5. Invariant review

- Advisor remains optional/local/default-off/advisory-only: untouched (no
  runtime path modified).
- `ResolvedToolSurface` authority: untouched; audit reads benchmark
  fixtures only.
- Span-packed ranker frozen: untouched (no ranker code or artifact
  modified).
- Historical train/dev/test/v2/v3 immutable: no corpus file modified; v3
  never loaded (asserted by `v3_is_absent_from_selection_inputs` and the
  hash-time v3 rejection).
- No remote model/telemetry/weight download: none; encoder never
  instantiated.
- Descriptor caches hold static metadata only: `RetrievalCacheKeyV2`
  carries descriptor identity + tokenizer/pooling/surface versions, never
  user context (tested).
- Evaluation-defect stop honored: gate untouched, no memorization
  training, corrective registered instead.

## 6. Failure and recovery review

Not applicable beyond determinism: the audit is a pure function of the
frozen corpus (dev-partition tripwire fails closed on corpus/partition
drift; preregistration hash fails closed on grid/field drift; missing
fixture candidates error instead of defaulting). No daemon paths, no
persistence, no concurrency, no partial-failure modes introduced.

## 7. Migration and compatibility review

No schema migration, no protocol change, no config change. The new JSON
receipt is additive under `assets/tool-advisor/`. `ToolAdvisorCandidate`
is unchanged (schema-plumbing disposition: experiment-local
representation, §6). Rollback is a two-commit revert with no data
implications.

## 8. Security review

No authorization surface touched. Query serialization inherits
`AdvisorContextV2` redaction (tested: `api_key=` payload redacted).
Schema extraction drops value-bearing keywords (examples, defaults,
const/enum) and caps bytes (tested). No secrets logged; receipt contains
only static tool metadata and benchmark contexts. Execution-ownership
guard passes (no subprocesses, no Git invocation; baseline SHA is a
caller-supplied string).

## 9. Documentation and operations

- Receipt: `assets/tool-advisor/retrieval-signal-m001-preregistration.json`.
- Corrective handoff: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/006-retrieval-evaluation-corrective.md`.
- Registry: retrieval-signal row and M001 row updated; 006 registered.
- Roadmap: M001 marked blocked; M006 registered; M002-M005 remain blocked
  with the blocker now named (006 relevance-target decision).
- No operator diagnostics or static guards beyond the in-code tripwires
  (dev-partition fingerprint, preregistration hash, v3 rejection).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Three gate-critical dev labels are supporting-workflow steps, not current-step needs (§1) | Retrieval gates cannot be honestly passed or failed until the relevance target is decided; M002-M005 stay blocked | 006 corrective must decide: all-plausible-workflow vs current-step-only vs graded recall, then re-baseline the frontier |
| medium | `tool-advisor-encoder-training` test profile does not compile on aarch64-darwin/rustc-1.89 (candle-core NEON f16) | Plan-literal verification command unrunnable on this host class; Linux CI unaffected | Future M002+ implementers must run the feature-gated suite on Linux CI or a compatible host before closing |
| low | `is_primary_highest_grade=true` on tied (2,2) rows | Field name suggests unique-primary; rows are tied-highest, resolved as supporting-step by sibling evidence | Consumers must read the rationale text, not the boolean alone; 006 re-audit may rename the field |

No critical findings. No findings indicate a defect in the shipped code.

## 11. Roadmap disposition

- M001: **blocked** — audit complete, preregistration frozen, §4 hard stop
  triggered. The representation contract (§5) and conditional M003 grid
  (§7) stand frozen for reuse once unblocked; they are not invalidated by
  the stop.
- M002: remains **blocked** — hard dependency is a *positive* M001, which
  this closure explicitly is not. Running the deterministic sweep against
  the current frontier would confound signal measurement with the three
  known-uninferable labels.
- M003: remains **blocked/conditional** — requires positive M001 plus a
  valid-negative M002; neither exists.
- M004: remains **blocked** — requires positive M002 or M003.
- M005: remains **blocked** — requires positive M004. The old
  order-invariance M005 stays blocked historical evidence, untouched.
- New M006 (retrieval-evaluation corrective): registered **ready** — owns
  the relevance-target decision and the frontier re-baseline that unblocks
  M002. This is the only downstream plan whose blocker can be cleared
  without new model evidence.

## 12. Registry updates

- `plans/registry.md` active-roadmaps row for the retrieval-signal
  experiment: M001 `ready` → `blocked`; current milestone now M006 ready,
  M002-M005 blocked.
- `plans/registry.md` dependency-ready table: M001 row `ready` →
  `blocked` with closure link; new M006 row `ready`.
- `plans/registry.md` retrieval-signal gate paragraph: M001-ready text →
  blocked-behind-evaluation-stop text with 006 pointer.
- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`:
  M001 `ready` → `blocked`; M006 milestone registered as ready with
  dependency edges (006 → unblocks M002).
- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001-*.md`:
  status line `ready for handoff` → `blocked`.
- M002-M005 plan files: status lines unchanged (`blocked`), still
  accurate; blockers now named in the roadmap (006 decision).
