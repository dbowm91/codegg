# Tool-Selection Advisor Data and Evaluation

The tool-selection advisor benchmark is a pure-Rust, offline consumer of the
existing tool discovery metadata. Its versioned `ToolAdvisorCase` records live
in `assets/tool-advisor/corpus.jsonl`; the fixture is reviewed seed data, not
user telemetry and not an execution authority.

Cases contain bounded task text, textual candidate descriptors, graded
relevance labels or an explicit `none` label, a leakage-prevention group,
semantic/task/tool-family metadata, generated-variant identity, and provenance.
`split_for(semantic_group)` assigns the complete semantic group to one stable
train/dev/test partition. Counterfactual variants share a candidate set while
changing the task/relevance labels, so a candidate-only memorizer cannot pass
the contextual gate. The frozen corpus also reports a tool-family holdout and
unknown-tool cases use synthetic names so a canonical-name memorizer cannot
satisfy the benchmark.

`codegg tool-advisor lint --dataset <jsonl> --json` validates the qualification
floors, duplicate/leakage metadata, counterfactual pairs, provenance, split
counts, and holdout fingerprint. The repository corpus is generated from
reviewed local templates and contains no private repository text or automatic
teacher output.

`codegg tool-advisor bench` reports the current keyword or BM25 ranking
baseline. It uses the same `ToolCatalog` scoring implementation as live
discovery through `ToolCatalog::rank_descriptors`; it does not register tools,
change disclosure, access the network, or affect normal startup.

To add a case, use a new group and ID, describe the semantic distinction being
tested in tags, avoid private repository content, and run the fixture validator
and benchmark tests. Future advisor runtime and training work must consume
these contracts rather than define incompatible parallel labels.

## Optional runtime (M002)

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

## Local training (M003)

The opt-in `tool-advisor-training` feature adds `codegg tool-advisor train`,
`eval`, and `inspect`. Training uses the same `hashed-linear-v1` scorer and
artifact writer as inference, with stable case-group splits and dataset/config
fingerprints. Each epoch writes an atomic checkpoint under a distinct run
directory; the selected artifact is replaced only after the run completes.
The repository includes `assets/tool-advisor/tiny-training.json` as a bounded
smoke configuration. Ordinary builds do not require the training feature.

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

## Integration and qualification (M005)

`tool_search` has four explicit modes: `off` (exact current behavior),
`observe` (score without changing the model-facing result), `rerank`
(reorder only the existing bounded shortlist), and `promote` (add at most two
high-confidence tools from the already policy-filtered deferred set). An
abstention probability of at least 0.5, a score below the configured
threshold, an advisor error, or a timeout leaves discovery unchanged.
Promotion is turn-local and never supplies execution arguments or widens
authority. Advisor mode does not enable telemetry.

`codegg tool-advisor qualify --model <artifact> --suite <jsonl>` emits the
pre-registered keyword/BM25/learned matrix by fixture tier, an unknown-tool
holdout, score-time and prompt-size bounds, and policy-negative coverage.
The suite must be held out from training; thresholds are selected from
development data only. The command is deterministic and offline, and its
results are qualification evidence rather than a default-on recommendation.
