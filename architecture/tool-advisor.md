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
`contextual-embedding-v1` artifact can be loaded by the same advisor
abstraction. It is a pure-Rust hashed-token embedding encoder with a learned
context/candidate interaction score, not a renamed linear artifact. The small
and medium capacity points are 5,242,881 and 15,728,641 parameters. Binary
artifacts carry an explicit manifest, tokenizer/version, dataset fingerprint,
weight digest, provenance, and license notice. A missing, corrupt, or
incompatible contextual artifact falls back to `NoopAdvisor` for the turn.
See `architecture/tool-advisor-framework-spike.md` for the bounded framework
comparison and selection rationale.

## Local training (baseline and contextual corrective)

The opt-in `tool-advisor-training` feature adds `codegg tool-advisor train`,
`eval`, and `inspect`. Training uses the same `hashed-linear-v1` scorer and
artifact writer as inference, with stable case-group splits and dataset/config
fingerprints. Each epoch writes an atomic checkpoint under a distinct run
directory; the selected artifact is replaced only after the run completes.
The repository includes `assets/tool-advisor/tiny-training.json` as a bounded
smoke configuration. The contextual configs in
`assets/tool-advisor/contextual-small-training.json` and
`contextual-medium-training.json` exercise the two capacity points through the
same Rust-only command. Ordinary builds do not require the training feature.

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
max_disclosure_schema_bytes, and the configured candidate count. The host
revalidates every predicted name against the final surface and excludes
required/never-reduce tools from learned visibility decisions. Denied,
disabled, plan-ineligible, non-callable, and parent-ceiling tools never enter
the advisor input.

`codegg tool-advisor qualify --model <artifact> --suite <jsonl>` emits the
pre-registered keyword/BM25/learned matrix by fixture tier, an unknown-tool
holdout, score-time and prompt-size bounds, and policy-negative coverage.
The suite must be held out from training; thresholds are selected from
development data only. The command is deterministic and offline, and its
results are qualification evidence rather than a default-on recommendation.
