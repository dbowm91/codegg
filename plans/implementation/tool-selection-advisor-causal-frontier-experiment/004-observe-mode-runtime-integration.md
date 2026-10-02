# Tool-Selection Advisor Causal Frontier M004 — Observe-Mode Runtime Integration

Status: implemented (closed positive — see `plans/closure/tool-selection-advisor-causal-frontier-experiment/004-status.md`)

Repository baseline: `aa21cfe1763d7ea00d11e582ed4108233e4088a9`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m004--observe-mode-request-preparation-integration`

Primary class: runtime integration/qualification.

## 1. Objective

Integrate the selected causal frontier into real request preparation in
**observe-only** mode while proving it cannot change provider tool definitions,
execution authority or discovery.

Selected frontier:

- M003 if M003 closed positive;
- otherwise M002.

## 2. Integration seam

Compute after final `ResolvedToolSurface` resolution and before provider
definitions are finalized.

The observe path may inspect:

- the immutable resolved surface;
- the bounded `CausalStateSnapshot`;
- native causal contract catalog.

It returns a recommendation/diagnostic only.

Provider definitions and `defer_loading` bits must be byte-for-byte identical
to the same turn with causal observe disabled.

## 3. Configuration

Add a default-off experimental config surface, for example:

```toml
[tool_advisor.causal_frontier]
mode = "off" # off | observe
```

Do not add active/promote mode in M004.

Config omission must be behaviorally identical to current main.

## 4. Runtime diagnostics

Record only bounded non-sensitive metrics:

- surface fingerprint;
- state/contract fingerprints;
- counts of admissible/inadmissible/uncontracted/required tools;
- selected canonical names when debug logging is explicitly enabled;
- evaluation latency;
- fallback/abstention reason.

No raw prompt, tool arguments, tool output, file content or secrets.

No remote telemetry.

## 5. Actual-call observation

Within an in-memory/session-local observation window, compare the next actual
tool call against the computed frontier.

Report:

- called tool was required/core;
- contracted+admissible;
- contracted+inadmissible;
- uncontracted fallback;
- absent from resolved surface.

This is evaluation only. An "inadmissible" observation must not block execution.

## 6. Replay qualification

Build deterministic request-preparation replay fixtures covering:

- no state;
- active Goal;
- active WorkPlan phases;
- artifacts;
- unresolved errors;
- failed tests;
- preview apply;
- uncontracted MCP/plugin definitions;
- denied/disabled/parent-ceiling tools;
- aliases/wire names.

Required invariants:

- definitions identical with observe off/on;
- resolved surface fingerprint identical;
- causal result canonical-name stable across aliases;
- unknown tool remains deferred/discoverable;
- hidden/denied tool never appears in causal result.

## 7. Runtime gates

Before M005:

- zero behavior delta in observe mode;
- zero authority violations;
- 100% fallback preservation;
- p95 frontier computation <=5 ms excluding state-store reads;
- no synchronous network I/O;
- no new background service;
- hosted default-feature CI green;
- selected M002/M003 offline gates reproduced.

## 8. Verification

```bash
cargo test -p codegg --lib request_preparation
cargo test -p codegg --lib tool_advisor::causal_frontier
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 9. Acceptance

Positive M004 proves the host projection can run safely in production-shaped
request preparation without changing behavior.

Positive M004 makes M005 ready.
