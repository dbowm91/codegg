# Tool-Selection Advisor Causal Frontier M005 — Active Disclosure and Trajectory Qualification

Status: implemented (closed with disposition B — see `plans/closure/tool-selection-advisor-causal-frontier-experiment/005-status.md`)

Repository baseline: `aa21cfe1763d7ea00d11e582ed4108233e4088a9`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m005--bounded-active-disclosure-and-trajectory-qualification`

Primary class: final qualification.

## 1. Objective

Qualify a bounded active causal-disclosure mode without turning causal metadata
into execution authority.

Active behavior is **promotion-only**:

- required/core/current contextual tools remain as today;
- causally selected deferred tools may be promoted immediately;
- contracted inadmissible deferred tools remain discoverable via `tool_search`;
- uncontracted tools remain discoverable;
- no tool becomes hidden/callable/authorized because of this layer.

## 2. Frozen active contract

Before any live/model qualification, freeze:

- selected M002/M003 frontier version/hash;
- contract catalog fingerprint;
- state ontology version;
- maximum promotions;
- schema byte budget;
- fallback behavior;
- required/core preservation rules;
- stateful qualification corpus;
- downstream model/provider set if live qualification is available.

No threshold tuning after trajectories begin.

## 3. Promotion limits

Initial hard bounds:

- max causally promoted deferred tools <=2;
- total promoted schema bytes <=16 KiB;
- no promotion outside resolved deferred universe;
- required/never-reduce preserved;
- no active change when state snapshot is insufficient;
- no active change for uncontracted-only candidate sets.

## 4. Structural qualification

Run the frozen M001 qualification partition plus fresh runtime-shaped cases.

Require:

- current-step tool preservation =1.00;
- uncontracted discovery preservation =1.00;
- authority violations=0;
- no increase in premature mutating exposure;
- median promoted deferred frontier <=2;
- p95 active projection <=5 ms excluding state I/O;
- provider schema budget respected.

## 5. Downstream trajectory qualification

If operator credentials/providers are available, run opt-in trajectories on a
frozen stateful suite with at least:

- one small/tool-fragile primary model;
- one stronger comparison model.

Compare causal mode vs existing palette using the same model/config/task seed
where the provider supports reproducibility.

Metrics:

- task success;
- wrong-tool call count;
- premature mutating call count;
- tool-search calls;
- tokens/tool-schema bytes;
- first-use tool correctness;
- unnecessary tool calls;
- latency.

Do not silently skip live evidence and call it qualified. If live prerequisites
are unavailable, close structurally positive but **not** eligible to unblock
the historical live-primary-model trajectory plan.

## 6. Fresh stateful holdout

Do not consume the retrieval v4 holdout.

Create a separate causal-frontier holdout after M004 freeze.

Minimum 160 scenarios with unseen combinations of:

- WorkPlan status/dependencies;
- acceptance/evidence state;
- artifacts;
- diagnostics/tests;
- previews;
- external/uncontracted tools;
- aliases;
- capability ceilings.

Every gold tool decision must cite the host fact/contract making it admissible.

## 7. Safety cases

Must include:

- denied mutating tool with an otherwise matching contract;
- parent-ceiling exclusion;
- stale LSP preview;
- no active WorkPlan;
- completed/cancelled WorkPlan;
- failed test then verification transition;
- unmet commit acceptance before/after test evidence;
- uncontracted MCP tool;
- forged/mismatched contract fingerprint;
- state snapshot drift between turns.

## 8. Disposition

- **A — qualified causal disclosure:** structural gates and required live
  trajectory gates pass.
- **B — structurally sound, live evidence unavailable/incomplete:** remain
  opt-in research; historical live-primary-model study stays blocked.
- **C — structural filtering useful but downstream model behavior regresses.**
- **D — no useful gain over existing palette.**
- **E — correctness/authority/contract-integrity failure.**

Only A may satisfy the "positive successor architecture" dependency of the
historical live-primary-model trajectory work, and its original
operator/provider/resource prerequisites still apply.

## 9. Contract trust boundary

No active mode may consume learned or external unverified contracts.

A future Contract2Tool-style derivation path requires its own plan and contract
attestation/integrity design before it can affect visibility.

## 10. Verification

```bash
cargo test --workspace --locked -- --test-threads=1
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Require green hosted CI on the frozen qualification head.

## 11. Acceptance

M005 closes with an explicit A/B/C/D/E disposition and machine-readable
structural/trajectory evidence.

No disposition other than A unblocks historical live advisor qualification.
