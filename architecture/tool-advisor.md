# Tool-Selection Advisor Data and Evaluation

The tool-selection advisor benchmark is a pure-Rust, offline consumer of the
existing tool discovery metadata. Its versioned `ToolAdvisorCase` records live
in `assets/tool-advisor/corpus.jsonl`; the fixture is reviewed seed data, not
user telemetry and not an execution authority.

Cases contain bounded task text, textual candidate descriptors, graded
relevance labels or an explicit `none` label, content-derived leakage
identity, semantic/task/tool-family metadata, generated-variant lineage, and
provenance. Split membership is derived from connected leakage components,
not from caller-supplied group strings: cases sharing a normalized
model-visible input signature, a generator/template lineage, a declared
semantic family, or an explicit manual leakage family join one component
(`build_leakage_groups`), and whole components are assigned to one stable
train/dev/test partition (`partition_cases`). True tool-family holdouts
(`family_holdout_partition`) exclude every component containing the held-out
family from optimizer and calibration input. Counterfactual variants share
an ordered candidate set while changing the task/relevance labels, so a
candidate-only memorizer cannot pass the contextual gate. The frozen corpus
also carries unknown-tool cases whose synthetic renames join a separate
`::unknown` lineage namespace that never enters training input.
`scripts/generate_tool_advisor_corpus.py` (frozen seed documented in the
C001 closure record) regenerates the 256-case fixture deterministically;
every context is unique, so group-count floors alone are never accepted as
leakage protection.

`codegg tool-advisor lint --dataset <jsonl> --json` validates the qualification
floors (256 cases, 128 leakage groups, 192 unique normalized inputs, 40
final-test leakage groups, no-tool/multi-tool/hard-negative/unknown-tool and
counterfactual minimums, 10 task families, 4 true family holdouts),
duplicate/leakage metadata, exact and normalized cross-split overlap (both
must be zero), template-lineage overlap (zero), same-input contradictory
labels, provenance, split counts, partition fingerprints, and the
family-exclusion matrix. The machine-readable `leakage` section of the lint
report is the C001 closure evidence. The repository corpus is generated from
reviewed local templates and contains no private repository text or automatic
teacher output. `split_for` remains only as a stable single-key hash for
legacy callers; qualification paths must use leakage-group partitions.

`codegg tool-advisor bench` reports the current keyword or BM25 ranking
baseline. It uses the same `ToolCatalog` scoring implementation as live
discovery through `ToolCatalog::rank_descriptors`; it does not register tools,
change disclosure, access the network, or affect normal startup.

To add a case, use a new group and ID, describe the semantic distinction being
tested in tags, avoid private repository content, and run the fixture validator
and benchmark tests. Future advisor runtime and training work must consume
these contracts rather than define incompatible parallel labels.

## Backend-neutral decision contract (M001)

`codegg_core::decision` defines the bounded v1 application decision contract:
Binary, exclusive Choice, ordinal Score, and multi-relevance Rank requests;
bounded state and candidate identities; explicit Answered, Abstained,
Unsupported, and Unavailable outcomes; backend capability/state reporting; and
a cancellable `DecisionEngine` interface with a caller-supplied deadline. Its
validation and canonical fingerprinting have no model, transport, or training
dependency. `NoopDecisionEngine` represents the ordinary off configuration.

The opt-in `src/decision.rs` adapter implements Binary (`noul`), Choice, and
integer-range Score over one question per request. Its reference and Ollama
profiles share the bounded `/v1/systemone` subset; Rank stays explicitly
unsupported. Score requires integral endpoints no more than nine levels apart.
Answers must use the generated question ID and requested option IDs. Remote
confidence is validated as a bounded diagnostic but is not promoted to
CodeGG confidence. The adapter uses Eggfetch with redirects off, 64 KiB
response bounds, pinned resolved addresses, and the caller deadline.
Reference endpoints require HTTPS and public addresses; Ollama permits only
loopback HTTP without credentials. Auth is resolved through CodeGG's existing
`AuthResolver`; unsupported auth modes fail closed.

`decision_engine.enabled` defaults to `false`; an off engine performs no DNS
lookup or request. Model discovery is an explicit operator call and does not
run in the background. The backend sends only bounded M001 state and question
payload; it never receives tool authority. HTTP, timeout, schema, and protocol
failures return `Unavailable` for deterministic host fallback.

The wire subset follows the [System One API reference](https://docs.system-one.dev/en/docs/api)
and the [Ollama System One endpoint](https://docs.ollama.com/api/systemone):
named questions with `choice`, `score`, or `noul`, a shared state, and answers
keyed by question ID. Model discovery is a direct `/v1/models` call only when
`discover_models` is enabled and an operator invokes `discover_models()`.

`src/tool_advisor/decision_adapter.rs` projects only a caller-supplied set of
entries already present on `ResolvedToolSurface`. It does not query a registry
or carry permission/broker references. The adapter and frozen
`assets/decision-runtime/compatibility-v1.jsonl` fixtures are additive
compatibility evidence; current live disclosure and model-specific runtime
ownership remain unchanged until later milestones close.

## Optional runtime (linear baseline and contextual corrective)

The runtime is an in-process `hashed-linear-v1` Rust scorer with a versioned
JSON artifact manifest. Manifest schema, tokenizer/context/candidate versions,
limits, calibration, provenance, and a SHA-256 weight digest are checked before
scoring. A missing, corrupt, incompatible, or repeatedly failing artifact
returns a diagnostic status and uses `NoopAdvisor`; it cannot fail an agent
turn. `off` is the configuration default. Reranking and promotion are
available only as explicitly configured experimental M005 modes.

`candidates_from_surface` projects only the already resolved policy-allowed
surface, so denied, disabled, plan-ineligible, and parent-ceiling tools cannot
enter the advisor input. The scorer has no broker, permission, registry, or
execution handle.

With the optional `tool-advisor` feature, a separate
`contextual-embedding-v2` artifact can be loaded by the same advisor
abstraction. It is a pure-Rust hashed-token embedding interaction scorer
with a learned context/candidate interaction score, not a renamed linear
artifact: mean-pooled hashed-token embeddings interact through a scaled dot
product plus bias. Until clean requalification says otherwise, documentation
must not claim more than that. The small and medium capacity points allocate
5,242,881 and 15,728,641 parameters, but the corpus touches only a few
hundred embedding buckets, so qualification reports trained/touched rows and
effective trained parameters rather than headline allocation; a compact
table configuration may match quality at a fraction of the bytes. Binary
artifacts carry an explicit manifest, tokenizer/version, dataset
fingerprint, weight digest, training discipline, dev-selected abstention
calibration, provenance, and license notice. Version 1 artifacts remain
loadable for compatibility but are always reported as legacy/unqualified;
only schema-v2 artifacts with partitioned discipline and `dev-grid-search-v1`
calibration may back qualification evidence. A missing, corrupt, or
incompatible contextual artifact falls back to `NoopAdvisor` for the turn.
Runtime abstention uses the serialized calibration
(`sigmoid((abstain_bias - max_score) / temperature)`); uncalibrated legacy
artifacts keep the exact historical `sigmoid(-top_score)` formula while
reporting their status. See `architecture/tool-advisor-framework-spike.md`
for the bounded framework comparison and selection rationale.

## Local training (baseline and contextual corrective)

The opt-in `tool-advisor-training` feature adds `codegg tool-advisor train`,
`eval`, and `inspect`. Training uses the same `hashed-linear-v1` scorer and
artifact writer as inference, with C001 leakage-group splits and
dataset/config fingerprints. Each epoch writes an atomic checkpoint under a
distinct run directory; the selected artifact is replaced only after the run
completes. Empty train or dev partitions are hard errors: the old all-case
training and calibration fallbacks are removed, and final-test metrics are
never computed during tuning. `codegg tool-advisor eval --partition
train|dev|test` (default `test`) scores one frozen partition explicitly;
`--partition all` is labeled diagnostic and must never back qualification
evidence. The repository includes `assets/tool-advisor/tiny-training.json`
as a bounded smoke configuration. The contextual configs in
`assets/tool-advisor/contextual-small-training.json`,
`contextual-medium-training.json`, and `contextual-compact-training.json`
exercise the two historical capacity points plus a compact table through the
same Rust-only command. Ordinary builds do not require the training feature.

Contextual training applies corrected binary-cross-entropy gradients
(sign and mean-pooling `1/n` factors pinned by finite-difference tests and a
tiny-overfit gate), consumes only the C001 train partition in the optimizer,
fits the abstention head on the dev partition only through a deterministic
temperature/bias grid search, and records per-split metrics (train/dev, never
test), the serialized calibration with uncalibrated reference values, and an
effective-capacity report (distinct buckets touched, rows changed, trained
parameter estimate, cold load, score latency). Each run writes a
machine-readable `<artifact>.training-report.json` sidecar next to the
artifact for review and requalification.

## Training-data lifecycle (M004)

`ToolAdvisorTrainingEvent` is separate from security/audit records. Capture is
`off` by default; explicit `local` capture writes one versioned, atomically
installed JSON record per event under the bounded local spool. `codegg
tool-advisor data status|inspect|export|purge` provides operator visibility and
retention control. Invalid records are quarantined rather than blocking the
agent.

Metadata consent, local content consent, remote metadata consent, and remote
content consent are independent. New events carry a host-owned
`TrainingConsentSnapshot`; legacy v1 booleans remain readable as audit fields
but cannot authorize transport. Content is absent from metadata-only events
and passes defense-in-depth redaction for common bearer/API-key forms before
persistence or export. Remote transport requires an explicit HTTPS endpoint,
an effective current host policy, and an event snapshot that grants the same
scope. Revocation is checked again at send time, so queued events cannot use
stale consent. Remote transport is never created by advisor enablement or
local capture. The HTTP adapter reuses the existing Eggfetch
client-construction seam; tests inject a fake transport so a default/off
configuration cannot make a network attempt.

## Integration and qualification (M003/M005)

`tool_search` has four explicit modes: `off` (exact current behavior),
`observe` (score without changing the model-facing result), `rerank`
(reorder only the existing bounded shortlist), and `promote` (add at most two
high-confidence tools from the already policy-filtered deferred set). In
addition, configured `promote` mode performs a turn-local pre-turn disclosure
projection after ResolvedToolSurface and before provider definitions are
finalized; it does not require a prior tool_search call. An abstention
probability of at least 0.5, a score below the configured threshold, an
advisor error, or a schema-budget overflow leaves the palette unchanged.
Promotion is turn-local and never supplies execution arguments or widens
authority. Advisor mode does not enable telemetry.

Pre-turn disclosure is bounded by max_promotions (hard-capped at 2),
max_disclosure_schema_bytes, and the configured candidate count. Candidate
construction is deferred-first: the full eligible deferred universe is built
from the resolved surface before any limit applies, and only then does a
deterministic BM25 preselection over the advisor-visible descriptor fields
narrow oversized universes to the neural budget
(`candidates_from_deferred_surface` + `preselect_candidates`; measured
preselection cost is single-digit milliseconds at 128 candidates, far below
provider latency). Resolved-surface position is never a ranking signal, so a
relevant deferred tool past the first-N surface window still reaches the
learned scorer. The host revalidates every predicted name against the final
surface and excludes required/never-reduce tools from learned visibility
decisions. Denied, disabled, plan-ineligible, non-callable, and parent-ceiling
tools never enter the advisor input.

`codegg tool-advisor qualify --model <artifact> --suite <jsonl>` emits the
pre-registered keyword/BM25/learned matrix by fixture tier, an unknown-tool
holdout, score-time and prompt-size bounds, and policy-negative coverage.
The suite must be held out from training; thresholds are selected from
development data only. The command is deterministic and offline, and its
results are qualification evidence rather than a default-on recommendation.

## Clean offline requalification (C004 disposition B)

The frozen C004 protocol (`assets/tool-advisor/c004-requalification.json`,
run with `codegg tool-advisor requalify --prereg …`) re-ran every baseline
and contextual variant on content-derived frozen partitions, true
family-excluded retraining runs, counterfactual/unknown/hard-negative/
no-tool slices, and a 64-tool candidate-recall fixture. Verdict: **B —
mechanically correct but no useful gain**. The corrected contextual variants
trail `hashed-linear-v1` on aggregate test MRR (0.46–0.60 vs 0.71), show no
context-sensitive slice gain without regression, abstain worse than the
trivial keyword baseline on no-tool cases, and transfer dev calibration
poorly to test abstention; preselector recall is 0.83 against a 0.98 gate.
The contextual scorer therefore remains a research/observe baseline: no
live-provider budget is spent on it, and the live M004 trajectory study
stays blocked pending a new model-architecture experiment. This demotion is
a verdict on the architecture, not on the corrected training mechanism,
which stays in place for any future experiment.

## Causal frontier experiment (M001 foundation)

The causal-frontier workstream replaces semantic retrieval with typed
host-owned state: a closed fact/outcome ontology
(`src/tool_advisor/causal_frontier.rs`), an additive `Tool::causal_contract`
seam plus the `ToolRegistry::causal_contract_of` registry seam, a bounded
`CausalStateSnapshot` projection, 20 static native pilot contracts, and a
frozen 168-case stateful benchmark
(`assets/tool-advisor/causal-frontier-v1.jsonl`) with its M002-gate
preregistration
(`assets/tool-advisor/causal-frontier-m001-preregistration.json`).

Invariants: `ResolvedToolSurface` remains the only per-turn capability
ceiling; causal metadata changes visibility only and never execution
authority. A missing contract means "not causally classifiable", never
"forbidden". Snapshots carry booleans, capped counts, and bounded typed ids
only — never raw output, prompts, file content, secrets, or transcripts.
No contract may be edited in response to qualification results without a
new experiment version: the preregistration records the contract-catalog
fingerprint, the benchmark fingerprint, the family-balanced dev (112) and
qualification (56) split, exact metric formulas with tie-breaking, and the
 frozen M002 gates (preservation 1.00/1.00, violations 0, reduction >= 0.50,
 median promotion <= 4, p95 <= 5 ms).

 ## Causal frontier experiment (M002 offline admissibility)

 M002 evaluates deterministic precondition filtering offline over the
 already-resolved eligible surface (`CausalFrontier::evaluate` in
 `src/tool_advisor/causal_frontier.rs`): contracted tools sort into
 admissible promotion vs. reasoned inadmissibility
 (`CausalInadmissibilityReason`), uncontracted tools stay in the fallback
 discovery universe, required/never-reduce tools bypass suppression, and
 withheld tools fail closed. Only admissible contracted tools deferred from
 `CORE_PALETTE` form the promotion set, used solely when structured signal
 exists (insufficient states abstain to the fallback universe). No runtime
 disclosure, broker, permission, or provider-definition behavior changes.

 Baselines on frozen dev (112): full eligible universe (no filter) and the
 `CORE_PALETTE` projection; frozen Signal V2 relevance labels are classified
 diagnostically only (contracted/uncontracted/admissible/unavailable). The
 untouched qualification partition (56) scores once; the machine-readable
 receipt (`assets/tool-advisor/causal-frontier-m002-result.json`, disposition
 A/D/E) is verified by test against live recomputation, with latency compared
 by gate rather than equality. Measured M002 outcome: disposition A —
 preservation 1.00/1.00, 0 violations, reduction 1.00, median deferred
 promotion 3, p95 ~0.03 ms. Positive M002 unblocks M004 observe integration
 and the optional M003 effect-path experiment.

## Causal frontier experiment (M003 structured effect-path frontier)

M003 tests a bounded effect-path refinement over the positive M002
admissibility frontier, running only when CodeGG already holds an explicit
structured desired outcome. Demand comes from typed host state alone:
frozen benchmark `desired_outcome` pairs validated against the
TestJob/Commit/Artifact/DelegatedRun-or-AgentRun mapping (plus unmet
acceptance derivation and explicitly armed preview-apply state at runtime).
Free-form prose is never consulted — a case whose rationale mentions
"test" but carries no typed demand abstains to M002 unchanged.

The planner (`find_minimal_effect_path` /
`plan_effect_path` in `src/tool_advisor/causal_frontier.rs`) treats
admissible contracted tools as directed transitions from current facts to
declared outcomes: breadth-first enumeration, maximum depth 3, no repeated
tool per path, no cycle expansion, deterministic lexicographic tie-break
after path length, no probabilistic score. Unknown/uncontracted tools are
outside the graph and stay fallback-discoverable. A path is advisory only.

Integrity: the effect-catalog fingerprint binds every pilot contract to
its live tool implementation id/version and input-schema fingerprint
(session-gated tools resolve through lazily constructed handles; no I/O),
so implementation or schema drift fails closed to the M002 fallback. No
external/MCP contract participates; no tool output can rewrite a
contract; every result carries per-tool contract provenance.

Measured M003 outcome on the 54 frozen structured-demand cases (dev 36 /
qual 18): disposition D (negative) — every demand finds a length-1 path
and the caller-visible median drops from 3 to 1 (reduction gate passes),
premature exposure stays 0, authority/fallback invariants hold, p95
~0.05 ms, but narrowing to demand producers hides plan/goal-state gold
tools (qual preservation 0.41, 32 false exclusions, 4 of 5 families lose
gold; only `artifact_recovery` preserves fully). The machine-readable
receipt (`assets/tool-advisor/causal-frontier-m003-result.json`) is
verified by test against live recomputation. M003 closes negative, so
M004 observe integration selects the positive M002 frontier.

 ## Causal frontier experiment (M004 observe-mode runtime integration)

 M004 integrates the selected frontier (M002 — M003 closed negative with
 disposition D) into real request preparation in **observe-only** mode
 (`src/tool_advisor/causal_observe.rs`). After final `ResolvedToolSurface`
 resolution and before provider definitions are finalized,
 `AgentLoop::build_tool_definitions` evaluates the frontier over the
 immutable surface plus bounded host-owned state and records diagnostics;
 provider definitions and `defer_loading` bits are byte-for-byte identical
 with observe disabled (proven live off-vs-on, plus a deterministic replay
 suite in `tests/causal_observe_replay.rs`).

 Config: default-off `[tool_advisor.causal_frontier] mode = "off" |
 "observe"` (`ToolAdvisorCausalFrontierConfig`); omission is identical to
 main, and no active/promote mode exists. Recorded metrics are fingerprints,
 counts, canonical names, latency, and the fallback/abstention reason only —
 no prompts, arguments, outputs, contents, or secrets, and no remote
 telemetry. Actual calls are compared against the session-local observation
 window at the broker (`observe_tool_call`) as diagnostics; an inadmissible
 observation never blocks execution. Two pilot facts stay absent at
 preparation time rather than guessed (structured failed-test status,
 turn-local LSP preview availability); absence can only abstain the
 frontier. M004 gates: zero behavior delta, zero authority violations, 100%
 fallback preservation, p95 <= 5 ms, no sync network I/O, no new background
 service. Positive M004 unblocks M005 bounded active disclosure.

 ## Causal frontier experiment (M005 bounded active disclosure)

 M005 adds the opt-in `active` mode (`src/tool_advisor/causal_active.rs`,
 frozen contract `assets/tool-advisor/causal-frontier-m005-freeze.json`):
 at most two causally admissible deferred tools promote from deferred to
 immediate per preparation, within 16 KiB total promoted schema bytes
 (serialized parameters JSON). Selection is greedy canonical-name order
 over the admissible contracted set intersected with the **resolution-time**
 deferred universe — deliberately not the static palette proxy, since a
 palette-core tool can still be deferred in a given preparation — skipping
 over-budget candidates. Required, core, and contextual immediacy are
 untouched (causal names only join the advisor promotion set);
 uncontracted tools are never promoted and stay discoverable via
 `tool_search`; insufficient signal abstains with no change, and frontier
 errors fail closed to the full fallback universe.

 Structural qualification (`tests/causal_active_m005.rs`): a fresh
 post-M004-freeze 284-scenario holdout
 (`assets/tool-advisor/causal-frontier-m005-holdout.json`, generated by
 `scripts/generate_causal_m005_holdout.py` from an independent
 transcription of the frozen semantics, every gold decision citing its host
 fact and contract) graded exact-match against the implementation, plus the
 frozen 56-case M001 qualification partition through the bound, plus a live
 active-without-state abstention check through
 `AgentLoop::build_tool_definitions`. Measured: current-step preservation
 1.00, uncontracted discovery 1.00, 0 authority violations, no premature
 promotion, median promotion 2 (bound of 2), p95 ~1.0 ms. Live model
 trajectories were unavailable (no operator provider credentials), so M005
 closes structurally positive (disposition B) and remains opt-in research:
 it does not unblock the historical live-primary-model trajectory work,
 which still requires disposition A with live evidence.
