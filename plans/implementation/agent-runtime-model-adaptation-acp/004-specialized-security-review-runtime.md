# Agent Runtime, Model Adaptation, and ACP Milestone 004 — Specialized Security-Review Runtime

Status: blocked — requires Milestone 003 closure

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-004--specialized-security-review-runtime`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`

Primary class: capability

## 1. Objective

Convert `runtime_kind = security_review` from metadata/prompt guidance into a bounded host-side security-review workflow. The runtime must collect deterministic evidence before model synthesis, use ordinary tool/permission/scheduler ownership, support explicitly approved read-only specialist children, and return a schema-validated report that separates evidence-backed findings from marker-only review prompts.

The goal is not production-grade autonomous penetration testing. It is a reliable defensive code-review agent for local development that reduces hallucinated findings and makes review coverage/evidence inspectable.

## 2. Dependencies

Hard dependencies:

- M001 canonical prompt/agent resolution;
- M002 resolved tool/capability surface;
- M003 bounded descendant delegation and lineage.

Existing interfaces:

- built-in `security-review` agent and prompt;
- `security` deterministic scanning tool/service;
- LSP `securityContext` operation and task-aware LSP context input;
- Git diff/status tooling;
- app events, subagent reports, artifact/context stores, and session projections.

No external vulnerability service is required.

## 3. Current implementation evidence

Re-audit at implementation time:

- `AgentRuntimeKind::SecurityReview` is parsed/resolved and logged but does not select a specialized execution path;
- `assets/agents/security-review.toml` declares read/LSP/security access and denies mutation;
- its prompt describes changed-file discovery, presets, deterministic scans, LSP evidence, correlation, and evidence-only findings;
- `src/agent/turn_runtime.rs` has task-aware LSP context metadata including `security_review_mode`;
- descendant execution currently attempts to parse generic text into `SubAgentReport` opportunistically rather than requiring a security schema;
- read-only filtering currently prevents nested specialists until M003 corrects it.

## 4. Invariants

- Security review remains read-only by default.
- Host preflight collects evidence; the model does not need to remember mandatory baseline steps.
- Risk markers, unsafe keywords, diagnostics, or scanner matches are review prompts, not findings by themselves.
- Findings require concrete code/evidence references and a reasoned reachability/exploitability statement.
- Specialist children inherit a narrower read-only authority ceiling and depth one by default.
- Full source bodies, secrets, hidden reasoning, and unbounded scanner output do not enter events/projections.
- The security runtime uses the ordinary `AgentLoop`, tool broker, permissions, cancellation, and scheduler.
- No exploit generation, offensive automation, or internet attack workflow is introduced.

## 5. Scope

### In scope

- Define a specialized-runtime hook/factory selected by resolved `AgentRuntimeKind`.
- Define `SecurityReviewInput`, `SecurityEvidenceBundle`, and `SecurityReviewReport` types.
- Resolve review scope from explicit changed files/hunks, active file, supplied commit/range, or bounded working-tree diff.
- Run deterministic preflight appropriate to project/language:
  - changed-line secret/credential patterns;
  - unsafe/FFI/process/network/input-validation risk markers;
  - dependency manifest/lockfile deltas;
  - LSP diagnostics, symbols, references/call expansion, and `securityContext` around changed hunks;
  - existing security tool presets.
- Record coverage and evidence gaps.
- Inject the bounded evidence bundle into the canonical prompt/context path.
- Require structured/schema-validated output.
- Support approved specialist child agents such as unsafe-Rust, dependency, authentication-boundary, or web-input review through configuration, not hard-coded branching.
- Correlate specialist reports into the parent report.
- Publish bounded progress and terminal summaries.

### Out of scope

- Automatic code modification/remediation.
- Dynamic exploit validation.
- Network scanning or live-target testing.
- Mandatory third-party scanners or vulnerability databases.
- Whole-repository exhaustive review on every invocation.
- Final team authorization or audit-retention policy.
- A new general security framework outside existing tools/LSP.

## 6. Required production changes

### Specialized runtime interface

Add a small typed hook around the ordinary loop, for example:

```rust
trait SpecializedAgentRuntime {
    async fn prepare(&self, ctx: &SpecializedRuntimeContext) -> Result<PreparedRuntime, Error>;
    async fn finalize(&self, output: AgentOutput, prepared: &PreparedRuntime)
        -> Result<SpecializedReport, Error>;
}
```

The exact API may differ. It must not duplicate provider streaming, tool execution, permissions, cancellation, or event handling.

### Evidence bundle

A bundle should include bounded, source-located records:

- review scope and diff identity;
- files/hunks examined;
- project/language preset decisions;
- deterministic matches with rule ID and location;
- LSP diagnostics/symbol/call evidence;
- dependency deltas;
- specialist child evidence;
- unavailable evidence and reasons;
- bundle fingerprint.

Large detail remains in artifact handles. The model receives enough context to assess findings without receiving unbounded logs.

### Typed report

Suggested report fields:

```rust
pub struct SecurityReviewReport {
    pub scope: SecurityReviewScope,
    pub findings: Vec<SecurityFinding>,
    pub review_prompts: Vec<SecurityReviewPrompt>,
    pub evidence_gaps: Vec<SecurityEvidenceGap>,
    pub coverage: Vec<SecurityCoverageRecord>,
    pub overall_confidence: Confidence,
}
```

Each finding requires severity, confidence, title, location, evidence references, reasoning, reachability/exploitability assessment, minimal remediation, and verification suggestion. Reject or downgrade records lacking required evidence rather than silently accepting them as confirmed findings.

### Delegation

- default max specialist depth: one;
- default direct specialists: small bounded count;
- only configured security specialist names may be called;
- specialists are read-only and usually cannot delegate;
- duplicate specialist requests reuse idempotent delegation behavior from M003;
- parent remains the report owner.

### Failure/cancellation

A failed optional scanner produces an evidence gap, not total failure. Failure to resolve review scope or loss of required repository/LSP identity should fail the review clearly. Cancellation stops preflight, specialists, and synthesis and returns one terminal state.

### Protocol/projection

Use existing subagent/progress/tool events plus an additive bounded security-review summary if needed. Do not project full evidence bundles by default; expose handle/fingerprint/counts and final bounded findings.

## 7. Ordered work packages

### A — Scope/evidence contract

- inventory security tool and LSP capabilities;
- define inputs, evidence record types, bounds, and report schema;
- add fixtures showing marker-only false positives and evidence-backed findings;
- define preset selection rules.

### B — Deterministic preflight

- implement changed-file/hunk resolution;
- run bounded security and dependency checks;
- collect LSP security context through explicit execution/workspace context;
- store large evidence behind artifacts;
- report unavailable evidence.

### C — Runtime dispatch and prompt integration

- dispatch by `runtime_kind` through the shared specialized hook;
- inject evidence into the canonical prompt/context compiler;
- request structured output when the provider supports it and validate locally regardless;
- prevent mutation tools from entering the surface.

### D — Specialist delegation

- add/configure example specialists;
- spawn through M003 policy;
- merge structured evidence;
- enforce depth/authority/budget limits.

### E — Finalization, events, docs

- validate/downgrade/reject unsupported findings;
- publish bounded report status;
- update security-review architecture and agent examples;
- document manual invocation and evidence limitations.

## 8. Failure, cancellation, restart, and contention semantics

- Preflight is cancellation-aware and publishes no partial bundle as final.
- Optional tool/LSP failure is represented as an evidence gap with bounded diagnostic.
- Required scope failure terminates before model invocation.
- Specialist cancellation/failure does not erase successful sibling evidence.
- Parent cancellation cascades to specialists.
- Concurrent reviews use ordinary scheduler/tool limits; no separate security pool.
- Restart may interrupt active review under existing transient-agent policy; a partial report is not marked complete.
- Evidence/artifact cleanup follows existing stores and lineage ownership.

## 9. Compatibility

- Existing `security-review` invocation remains available.
- Existing prompt content becomes guidance layered over deterministic host behavior.
- Existing `security` and LSP tools remain callable through their canonical interfaces.
- Generic agents may still call security tools; only `runtime_kind` invokes the specialized preflight/report contract.
- Custom agents extending `security-review` inherit runtime kind unless explicitly and validly changed.

## 10. Required tests

Focused:

- scope resolution from files/hunks/range/working tree;
- preset classification;
- deterministic evidence normalization/bounds;
- marker-only versus finding validation;
- report schema validation;
- optional scanner/LSP failure as evidence gap;
- required scope failure;
- specialist allowlist/depth/authority;
- cancellation and duplicate child behavior;
- no mutation surface.

Production-shaped:

- unsafe Rust change with LSP/call evidence;
- dependency manifest change with bounded dependency review;
- benign marker that remains a review prompt;
- two specialist children merged into one parent report;
- cancellation during preflight and during synthesis.

Negative/security:

- secret-like fixture is redacted in events/projections;
- full evidence body is handle-backed when oversized;
- model output cannot invent an evidence reference and pass validation;
- specialist cannot gain write/shell-mutate/commit authority;
- no offensive tool or exploit instruction is added.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test security::
cargo test lsp::security
cargo test agent::worker
cargo test --test subagent
cargo check --workspace
```

Add one focused security-runtime integration target. Run one broad local library suite at handoff; do not add external scanners or a multi-platform CI matrix.

## 12. Acceptance criteria

- `SecurityReview` selects real host-side preflight/finalization behavior.
- Review scope and deterministic evidence are assembled before synthesis.
- Findings are schema-valid and evidence-backed; marker-only records stay separate.
- Optional evidence failures are explicit.
- Approved read-only specialists work through bounded nested delegation.
- Mutating authority is absent.
- Reports/events are bounded and redact private material.
- Ordinary agent/tool/scheduler ownership remains intact.

## 13. Stop conditions

Stop if:

- required security evidence needs a new external service or network scanner;
- LSP/security tool APIs cannot provide bounded source-located evidence without reopening their ownership;
- structured output requires a provider-specific redesign that belongs to M007/M008;
- specialist mutation/worktree behavior becomes necessary;
- final team authorization must be invented rather than consumed as a seam.

## 14. Closure evidence

Include:

- scope/evidence/report schemas;
- marker-only and evidence-backed fixture results;
- preflight coverage and evidence-gap examples;
- specialist delegation/authority evidence;
- cancellation/resource evidence;
- focused and broad local verification results;
- known scanner/LSP limitations;
- closure recommendation.
