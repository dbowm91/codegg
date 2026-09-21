# Tool-Selection Advisor Data and Evaluation

The tool-selection advisor benchmark is a pure-Rust, offline consumer of the
existing tool discovery metadata. Its versioned `ToolAdvisorCase` records live
in `assets/tool-advisor/corpus.jsonl`; the fixture is reviewed seed data, not
user telemetry and not an execution authority.

Cases contain bounded task text, textual candidate descriptors, graded
relevance labels or an explicit `none` label, a leakage-prevention group, and
provenance. `split_for(group_id)` assigns the complete group to one stable
train/dev/test partition. Unknown-tool cases use synthetic names so a name-only
memorizer cannot satisfy the benchmark.

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
turn. M002 accepts only `off` and `observe`, with `off` as the configuration
default. Reranking and promotion are reserved for M005.

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

Metadata consent, local content consent, remote consent, and remote content
consent are independent. Content is absent from metadata-only events and passes
defense-in-depth redaction for common bearer/API-key forms before persistence or
export. Remote transport requires an explicit HTTPS endpoint and is never
created by advisor enablement or local capture. The HTTP adapter reuses the
existing Eggfetch client-construction seam; tests inject a fake transport so a
default/off configuration cannot make a network attempt.
