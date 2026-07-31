# Agent Runtime, Model Adaptation, and ACP Milestone 013 — Specialized Runtime Finalization and Research Coordination

Status: blocked — requires Milestone 012 strict closure

Repository baseline: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`

Source corrective addendum:

- `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-013--specialized-runtime-finalization-and-research-coordination`

Historical plans corrected by this milestone:

- `plans/implementation/agent-runtime-model-adaptation-acp/004-specialized-security-review-runtime.md`
- `plans/implementation/agent-runtime-model-adaptation-acp/005-specialized-research-runtime.md`

Primary class: capability/correctness

## 1. Objective

Make specialized security and research runtimes authoritative at both preparation and finalization. Security output must be locally parsed and evidence-validated before it is accepted as a report. Research must execute its bounded plan through host-owned child coordination, collect typed evidence reports, deduplicate and validate sources/claims/citations, and locally validate the final report before completion.

The milestone must preserve one ordinary `AgentLoop`, scheduler, tool broker, permission checker, subagent pool, cancellation path, and event/projection authority. It must not create a second orchestration framework or turn research into an unbounded workflow engine.

## 2. Dependencies

Hard dependency:

- Milestone 012 strict closure, so ACP and projection consumers observe a stable one-turn/one-terminal contract while specialized terminal output handling changes.

Existing foundations:

- `src/security/runtime.rs` defines `SecurityEvidenceBundle`, `SecurityReviewReport`, `report_schema`, and `validate_report`;
- `src/research/runtime.rs` defines bounded request classification, plans, child roles, source/evidence/claim/report types, source deduplication, and report validation;
- `src/agent/turn_runtime.rs` prepares security evidence or a research plan before constructing the ordinary loop;
- `SubAgentPool` and `TaskTool` provide bounded descendant execution;
- structured output can be requested through `ResponseFormat::JsonSchema` but providers may ignore it;
- session projections and app events can carry bounded progress and terminal summaries.

No external scanner, search key, browser, or paid provider is required for implementation or closure.

## 3. Current implementation evidence

Re-audit at implementation time. At the reviewed baseline:

- security preflight is host-owned and injected into the prompt, but final model text is not parsed into `SecurityReviewReport` and `validate_report` is not called by the production turn path;
- research runtime types and pure validation helpers exist, but production only builds a plan and inserts plan text into the prompt;
- planned research child tasks are not host-spawned or awaited by a research coordinator;
- child output is generally parsed opportunistically as `SubAgentReport`, not required to be `ResearchEvidenceReport`;
- source deduplication, claim/evidence relationships, conflicts, citation validation, and minimum-evidence policy are not authoritative production completion gates;
- `ResponseFormat::JsonSchema` is requested for security/research, but local validation is not wired after the provider completes;
- `AgentLoop::run` returns events, while the root runtime publishes completion based on loop success rather than specialized finalizer success.

## 4. Invariants that must not regress

- Provider structured-output compliance is advisory; local parsing and validation are authoritative.
- Invalid or unsupported security findings are rejected or downgraded to review prompts/evidence gaps.
- Research children return bounded typed evidence; they do not own final synthesis or completion.
- The parent specialized runtime owns source deduplication, conflict detection, citation validation, and final report acceptance.
- Child authority cannot exceed parent/session/config/hard ceilings.
- Security and built-in research remain read-only.
- All child execution uses the existing shared pool and ordinary scheduler/tool/permission paths.
- Cancellation propagates through preparation, child work, root synthesis, and finalization.
- Optional evidence failure is explicit; required validation failure prevents a successful specialized terminal state.
- Reports/events/projections are bounded and omit private reasoning, credentials, and full source/scanner bodies.
- Generic agents may continue using security/research tools without invoking specialized runtime finalization.

## 5. Scope

### In scope

- Introduce a minimal typed specialized runtime lifecycle around the ordinary loop: prepare, optional coordinate, and finalize.
- Capture the final visible model output and provider/tool events needed by finalizers.
- Parse security output into `SecurityReviewReport` and call `validate_report` against the prepared bundle.
- Define behavior for malformed security JSON, unsupported findings, and provider schema noncompliance.
- Execute `BoundedResearchPlan` child tasks through the existing `SubAgentPool` or scheduler-backed task seam.
- Require typed `ResearchEvidenceReport` from research children.
- Aggregate child sources/evidence/claims/limitations with explicit bounds and deduplication.
- Define minimum-evidence and partial-failure policy by request kind.
- Perform root synthesis through the ordinary model turn using the prepared evidence ledger, then parse and locally validate `ResearchReport`.
- Publish bounded progress/terminal summaries and optional artifact handles for large evidence.
- Add deterministic captured/local integration fixtures.

### Explicitly out of scope

- New browser automation, crawler, vector database, knowledge graph, or academic systematic-review system.
- Arbitrary recursive research trees or dynamic workflow scripting.
- Automatic code remediation from security/research findings.
- Live exploit validation, network scanning, or external vulnerability services.
- New durable AgentRun schema or restart recovery.
- Provider-specific report parsers outside the common structured-output/finalizer seam.
- Mandatory live search/model testing.

## 6. Required production changes

### Specialized runtime lifecycle

Add a small host-owned abstraction selected by resolved `AgentRuntimeKind`, for example:

```rust
trait SpecializedRuntime {
    type Prepared;
    type FinalReport;

    async fn prepare(&self, ctx: &RuntimeContext) -> Result<Self::Prepared, Error>;
    async fn coordinate(
        &self,
        prepared: &Self::Prepared,
        ctx: &RuntimeContext,
    ) -> Result<CoordinationOutput, Error>;
    fn finalize(
        &self,
        prepared: &Self::Prepared,
        coordination: &CoordinationOutput,
        model_output: &str,
    ) -> Result<Self::FinalReport, Error>;
}
```

The exact shape may differ. Keep it small and internal. It must not duplicate provider streaming, tool execution, permissions, scheduler admission, cancellation, or event publication.

### Agent-loop output seam

Provide a bounded terminal result from `AgentLoop::run`, or an adjacent collector, that includes:

- final public assistant text;
- terminal reason;
- bounded usage summary;
- tool/child completion summary when needed;
- no hidden reasoning body.

Existing callers may retain a compatibility event vector, but specialized finalizers need a typed reliable final-output seam. Root turn completion must occur only after the specialized finalizer accepts the output.

### Security finalization

- Parse final public output as `SecurityReviewReport`.
- Enforce size, collection, string, location, and evidence-reference bounds before/while parsing.
- Call `validate_report(report, bundle)`.
- Move rejected findings to explicit evidence gaps or review prompts according to the existing contract.
- Treat malformed/unparseable output as a specialized validation failure, not successful ordinary completion.
- Permit one bounded corrective model retry only if already supported by the generic recovery/adapter policy; do not add an unbounded security-specific loop.
- Publish a bounded report summary: finding count by severity, review-prompt count, evidence-gap count, coverage, confidence, bundle fingerprint, and optional artifact handle.

### Research coordination

Implement a bounded coordinator that consumes `BoundedResearchPlan`:

- quick lookup: no child spawn; root uses ordinary approved search/research tools and final validation;
- direct investigation: at most the planned repository/spec investigator;
- multi-source: at most the existing bounded source scouts and claim verifier;
- child targets come from configured/approved agent names or fixed internal role mappings that cannot grant authority;
- every child request carries explicit workspace, denied tools, allowed paths, parent model, depth, timeout, and tool-call budget;
- child responses must parse as `ResearchEvidenceReport`; malformed reports fail that branch explicitly;
- duplicate child requests use existing idempotency and do not create extra work.

Do not recursively invoke the full research specialized runtime for scout children. Children gather evidence only.

### Evidence aggregation

- Normalize and deduplicate source identity.
- Bound sources, evidence, claims, conflicts, excerpts, and limitations.
- Reject unknown source/evidence references.
- Preserve support/contradict/uncertain relations.
- Detect duplicate or conflicting claims deterministically where IDs/text normalize equivalently.
- Record branch failure and unavailable provider/tool conditions as limitations.
- Keep full source bodies and large excerpts behind existing artifacts/handles.

### Minimum-evidence policy

Define a conservative typed policy:

- quick lookup may complete with one valid source or explicit no-source limitation when the answer is repository-local and source-backed by files;
- direct investigation requires the planned repository/spec evidence branch or a clear failure;
- multi-source requires evidence diversity appropriate to the plan; if minimum evidence is not met, final status is incomplete/failed rather than confident synthesis;
- the claim verifier cannot invent sources or widen evidence.

### Research finalization

- Build the root synthesis context from aggregated evidence, not raw child essays.
- Parse final output into `ResearchReport`.
- Call local `validate_report` and additional bounds/conflict checks.
- Reject fabricated citations and unknown evidence references.
- Distinguish direct support, inference, conflict, limitation, and unresolved questions.
- Publish bounded counts/fingerprint/handle, not complete retrieved documents.

## 7. Ordered work packages

### Work package A — Terminal output and specialized lifecycle seam

- inventory root/child `AgentLoop::run` consumers;
- define typed terminal output compatible with ordinary callers;
- add specialized prepare/coordinate/finalize dispatch;
- ensure finalizer failure changes the native terminal result.

Acceptance evidence:

- ordinary agents remain behavior-compatible;
- specialized success cannot be published before local finalization;
- hidden reasoning is absent from finalizer input.

### Work package B — Security finalization

- wire bounded JSON parsing and `validate_report`;
- define malformed/unsupported finding behavior;
- publish bounded validated summary;
- add benign-marker, unsupported-finding, malformed-output, and valid-finding fixtures.

Acceptance evidence:

- out-of-scope or evidence-free finding cannot pass;
- provider schema noncompliance cannot silently succeed;
- marker-only fixture remains a review prompt/evidence gap.

### Work package C — Research child execution

- map bounded plan tasks to approved child agents;
- spawn/await through shared pool;
- require typed child evidence reports;
- enforce read-only authority, depth, timeout, and budgets;
- collect partial failures without unbounded retry.

Acceptance evidence:

- multi-source plan executes no more than the planned bounded tasks;
- child cannot mutate or delegate beyond its ceiling;
- malformed child report is explicit and does not become evidence.

### Work package D — Evidence ledger and synthesis

- deduplicate sources and evidence;
- validate claim/evidence/source links;
- identify conflicts/limitations/minimum-evidence failure;
- feed bounded ledger to root synthesis;
- validate final `ResearchReport` locally.

Acceptance evidence:

- fabricated citation fails;
- duplicate sources coalesce;
- conflicting evidence remains visible;
- insufficient evidence cannot produce a successful confident report.

### Work package E — Cancellation, events, docs, and closure

- cascade cancellation through coordination and synthesis;
- publish bounded phase/count/terminal summaries;
- update agent/security/research architecture;
- create M013 closure record only after independent review;
- promote M014 only on strict closure.

## 8. Failure, cancellation, restart, and contention semantics

- Preparation failure prevents model invocation when required scope/evidence cannot be established.
- Optional scanner/search/LSP failure becomes an evidence gap or limitation.
- Required child failure may produce incomplete research only when minimum-evidence policy permits; otherwise the specialized turn fails.
- Malformed child or final model JSON is a typed validation failure.
- Parent cancellation stops queued/running child tasks and root synthesis through existing cancellation ownership.
- Sibling child failure does not erase valid sibling evidence.
- Duplicate source retrieval is coalesced within one root plan where safe.
- Concurrent specialized roots use ordinary global pool/scheduler limits; no new specialized worker pool.
- Daemon restart may interrupt transient work; no partial report is marked complete.
- Finalizer failure publishes one terminal failure and no completed report.

## 9. Compatibility and migration

- Existing generic security/research tools remain callable.
- Existing `security-review` and `research` agent names remain valid.
- Agent TOML and provider DTOs remain additive-compatible.
- A typed `AgentLoop` terminal result may require internal caller updates but should not change native external protocol unless a bounded report summary is added.
- No durable storage migration is required.
- Existing projection consumers may ignore additive specialized summary fields/events.

## 10. Required tests

### Security tests

- valid evidence-backed report passes;
- finding outside prepared target fails/downgrades;
- finding without source location/evidence fails/downgrades;
- marker-only result remains non-finding;
- malformed/oversized JSON fails;
- provider ignores schema but local validator catches output;
- cancellation during preparation and synthesis.

### Research planning/coordination tests

- quick lookup spawns no child;
- direct investigation spawns at most one approved child;
- multi-source spawns bounded non-overlapping roles;
- child authority/path/depth/tool/time limits;
- malformed child evidence report;
- partial branch failure;
- cancellation cascades to accepted children;
- duplicate child identity is reused/rejected consistently.

### Evidence/finalization tests

- URL/path source normalization and deduplication;
- duplicate evidence/claim handling;
- supports/contradicts/uncertain preservation;
- unknown source/evidence reference rejection;
- minimum-evidence enforcement;
- fabricated citation rejection;
- conflict and unresolved-question retention;
- large source body becomes handle/bounded excerpt.

### Production-shaped fixtures

- benign security marker with no confirmed finding;
- evidence-backed unsafe Rust/security finding;
- repository architecture investigation with one typed child report;
- comparative multi-source research with duplicate and conflicting sources;
- unavailable web backend with explicit limitation and no fabricated answer.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets
cargo test -p codegg security::runtime
cargo test -p codegg research::runtime
cargo test --test security_review_runner -- --test-threads=4
cargo test --test security_review_receipt -- --test-threads=4
cargo test --test subagent -- --test-threads=4
cargo test --test agent_loop_harness -- --test-threads=4
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
scripts/check_projection_disclosure.sh
```

Add one focused specialized-finalization integration target if the existing fixtures cannot exercise the production seam. Do not add live-search CI, external scanners, or a broad provider matrix.

## 12. Documentation updates

- `architecture/agent.md`: specialized prepare/coordinate/finalize seam;
- security architecture/tool documentation: local finalization and rejection behavior;
- research architecture/tool documentation: host-owned child/evidence coordination and minimum-evidence policy;
- projection documentation if an additive bounded specialized summary is introduced;
- corrective addendum, registry, and M013 closure record.

## 13. Acceptance criteria

- Specialized success is published only after local finalizer acceptance.
- Security output is parsed and validated against prepared evidence in production.
- Unsupported findings cannot leave as confirmed findings.
- Research plans execute bounded approved child roles through the shared pool.
- Children return typed evidence records, not authoritative final answers.
- Sources/claims/evidence/conflicts/citations are host-validated.
- Minimum-evidence and partial-failure behavior is deterministic.
- Cancellation reaches preparation, children, synthesis, and finalization.
- Reports/events are bounded and private content remains absent.
- Ordinary agent runtime ownership is not duplicated.

## 14. Stop conditions

Stop and report if:

- correct finalization requires a provider-specific execution path outside the common structured-output seam;
- research requires browser automation, a crawler, persistent knowledge graph, or arbitrary recursive workflows;
- child execution cannot use the shared pool/scheduler without reopening its authority model;
- typed final output requires exposing provider-private reasoning;
- final reports require a new durable report database rather than existing artifacts/handles;
- mutation-capable specialists or worktree allocation become necessary.

## 15. Required closure evidence

The closure record must include:

- specialized lifecycle and terminal-output contract;
- security malformed/unsupported/valid report evidence;
- research plan-to-child execution trace with bounds;
- typed child evidence and aggregation examples;
- source dedupe/conflict/citation/minimum-evidence evidence;
- cancellation and authority evidence;
- focused command results and exact commit hashes;
- remaining low-severity limitations;
- explicit recommendation to promote or block Milestone 014.