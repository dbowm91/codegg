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
