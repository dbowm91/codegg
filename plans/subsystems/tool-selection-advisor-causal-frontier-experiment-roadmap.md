# Tool-Selection Advisor Causal Frontier Experiment Roadmap

Status: active

Repository planning baseline: `aa21cfe1763d7ea00d11e582ed4108233e4088a9`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`
- `architecture/agent-tool-surface.md`

Closed predecessor evidence:

- `plans/closure/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/003-status.md`
- `plans/closure/tool-selection-advisor-late-interaction-retriever-experiment/001-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-closure-corrective/001-status.md`

## 1. Why a new architecture line is warranted

The prior advisor experiments have now tested and rejected several variants of
the same semantic-retrieval family:

- keyword/BM25 retrieval;
- frozen pooled MiniLM retrieval;
- enriched Signal V2 descriptors;
- lexical+dense fusion;
- learned projections over pooled MiniLM vectors;
- frozen token-level MiniLM MaxSim late interaction.

The late-interaction successor closed with disposition D: exact MaxSim recovered
only 29/53 inferable dev labels and contributed no gain over the lexical arm in
the fixed union. Continuing to rearrange the same MiniLM representation is not
well supported by evidence.

The next experiment therefore changes **the source of information** rather than
the embedding geometry.

CodeGG already owns structured state that ordinary retrieval systems do not:

- immutable per-turn `ResolvedToolSurface` authority;
- `ToolContract` execution/effect metadata;
- active Goal state;
- durable WorkPlan/WorkItem dependencies, status, acceptance and evidence refs;
- context-ledger touched files, tests, unresolved errors and artifact handles;
- turn-local LSP preview availability;
- existing host-derived contextual disclosure.

The new line asks whether these typed facts can exclude tools that are
premature or causally inadmissible before semantic discovery/ranking.

## 2. Research basis

The research is architectural guidance, not a dependency.

- ToolChoiceConfusion / Causal Minimal Tool Filtering argues that semantic
  relevance is insufficient and filters the visible tool menu with
  precondition/effect contracts:
  - https://arxiv.org/abs/2606.06284
- ToolMenuBench evaluates tool-menu construction under state-dependent tasks,
  distractors and risk exposure and reports large downstream differences
  between menu strategies:
  - https://arxiv.org/abs/2606.15508
- Contract2Tool shows that schemas/documentation/traces can eventually support
  contract derivation, but this experiment does **not** adopt learned contract
  inference:
  - https://arxiv.org/abs/2606.07904
- ContractGuard highlights that effect/precondition integrity becomes a
  load-bearing trust assumption for causal gating:
  - https://arxiv.org/abs/2606.18550
- SING and Procedural Graphs provide evidence that evolving task state and
  procedure structure can outperform static one-shot discovery:
  - https://arxiv.org/abs/2606.16591
  - https://arxiv.org/abs/2609.09153

## 3. Architectural boundary

The experiment adds a host-owned causal **visibility** layer, never a new
execution authority.

```text
ToolRegistry
   |
   v
ResolvedToolSurface              <- canonical authority ceiling
   |
   v
CausalStateSnapshot              <- host-owned state only
   |
   v
ToolCausalContract evaluation
   |
   +--> admissible contracted frontier
   +--> uncontracted/unknown fallback universe
   |
   v
existing disclosure / tool_search / provider definitions
```

The causal frontier may recommend or suppress **promotion** of deferred tools.
It may not register tools, grant capability, bypass permission/broker policy,
or make an uncontracted tool undiscoverable.

## 4. Deliberate scope choice

This workstream starts with causal **admissibility**, not full natural-language
planning.

M001/M002 use only host-owned facts such as:

- active goal/work plan/current item;
- work item status/dependencies;
- unmet host-evidence acceptance kinds;
- artifact handles;
- touched-file presence;
- test evidence / failing-test evidence;
- unresolved errors;
- LSP preview availability;
- backend availability already represented by the resolved surface.

No LLM or embedding model converts arbitrary prose into causal facts.

M003 may add a bounded effect-path frontier only when the desired outcome is
explicitly derivable from structured WorkPlan acceptance/evidence state. If
there is no structured causal demand, it abstains to the M002 admissibility
frontier.

## 5. Contract integrity policy

CodeGG already has `ToolContract`, but its `ToolEffectClass` is too coarse
for causal selection. The experiment adds **planning metadata**, not execution
policy.

Preferred seam:

```rust
trait Tool {
    // existing methods unchanged
    fn causal_contract(&self) -> Option<ToolCausalContract> { None }
}
```

A sibling catalog is preferred over changing broker authorization semantics.

Rules:

- native contracts are source-controlled and fingerprint-bound to tool
  identity/implementation version;
- missing contract means "not causally classifiable", never "forbidden";
- MCP/plugin/external tools remain discoverable unless they have a trusted
  future contract source;
- inferred/LLM-generated contracts are out of scope;
- a causal contract never overrides `ResolvedToolSurface`, broker, permission,
  sandbox or caller policy.

## 6. Milestone graph

```text
M001 typed state + causal-contract foundation + benchmark prereg
  |
  v
M002 offline causal-admissibility frontier
  | negative ---------------------------> terminal negative
  | positive
  +-------------------+
  |                   |
  v                   v
M003 optional      M004 observe-mode runtime integration
effect-path           ^
frontier              |
  | positive/useful --+
  | negative: M002 remains candidate
  v
M004
  |
  v
M005 bounded active disclosure + trajectory qualification
```

M003 is an optional enhancement. A positive M002 can advance to M004 even if
effect-path reduction proves unnecessary or negative.

## 7. Milestones

### M001 — Typed causal contracts, state projection, and benchmark preregistration

Plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/001-typed-contracts-state-and-benchmark.md`

Status: **ready**.

Define the closed state/effect ontology, add the additive contract seam, build a
host-only state projection, annotate a bounded native pilot set, and freeze a
stateful benchmark before measuring a frontier.

### M002 — Offline causal-admissibility frontier

Plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/002-offline-causal-admissibility-frontier.md`

Status: blocked on positive M001.

Evaluate deterministic precondition filtering before semantic ranking. Unknown
contracts remain fallback/discoverable.

### M003 — Structured effect-path frontier

Plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/003-structured-effect-path-frontier.md`

Status: blocked/optional on positive M002.

Use only explicit structured WorkPlan demands to compute a bounded causal path.
No free-text intent model.

### M004 — Observe-mode request-preparation integration

Plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/004-observe-mode-runtime-integration.md`

Status: blocked on positive M002.

Integrate the selected frontier after `ResolvedToolSurface` in observe-only
mode, with no provider-menu behavior change.

### M005 — Bounded active disclosure and trajectory qualification

Plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/005-active-disclosure-trajectory-qualification.md`

Status: blocked on positive M004.

Promote only a bounded causally admissible deferred frontier while preserving
core/required tools and `tool_search`; qualify structural safety plus opt-in
model trajectories.

## 8. Durable invariants

- `ResolvedToolSurface` remains the only per-turn capability ceiling.
- Causal filtering changes visibility only.
- Required/never-reduce tools remain visible.
- An uncontracted tool is never removed from discovery/callability because of
  missing causal metadata.
- Hidden/denied/disabled/parent-ceiling tools can never be restored.
- No remote telemetry.
- No model weights or new inference runtime.
- No parsing/storing chain of thought.
- State projection contains only host-owned bounded facts.
- Contract/effect metadata is versioned, deterministic and fingerprinted.
- No contract is inferred from untrusted tool output in this workstream.

## 9. Primary success criteria

The exact benchmark thresholds are frozen in M001 before M002 measurement, but
the workstream requires at least:

- 100% preservation of gold current-step tools on the stateful native benchmark;
- 0 authority violations;
- 100% preservation of uncontracted tools in the fallback discovery universe;
- >=50% reduction in premature/inadmissible contracted-tool exposure versus the
  same resolved surface without causal filtering;
- median causally promoted deferred frontier <=4 tools when a structured state
  signal exists;
- abstention/fallback when the state is insufficient rather than guessing.

M005 additionally requires no degradation in downstream task success relative
to the existing palette on its frozen trajectory suite.

## 10. What is explicitly deferred

Not part of this workstream:

- learned/LLM-generated causal contracts;
- automatic contract derivation from traces;
- signed third-party contract distribution;
- graph learning/evolving procedural graphs;
- semantic embedding retraining;
- another MiniLM retrieval experiment;
- hiding uncontracted MCP/plugin tools;
- using causal metadata as execution authorization.

If static native contracts prove useful, external contract derivation and
contract-attestation can become a separate successor.
