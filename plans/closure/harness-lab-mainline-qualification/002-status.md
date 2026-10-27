# Harness Lab Mainline Qualification H002 — Closure Status

Status: closed for the v2 benchmark contract; historical C004 live non-exercise remains unchanged.
Implementation: versioned ADR-0040 controls, typed ACP evidence and an independently exercised native fixture.

## Result

The v1 autonomy manifest and historical C004/C005 records are byte-for-byte unchanged. The independent v2 manifest has SHA-256 recorded by `recovery_controls_v2.load_suite()` and refuses unknown fields, duplicate IDs, unsupported versions, mixed-version score pairs and digest mismatches. `not_exercised` and `unknown` cannot enter score acceptance. Action identity includes tool, canonical ACP argument digest, typed status and harness-measured workspace revision. Output text is ignored. Stop owner and recovery-rung evidence are explicit; the current ACP surface does not expose a rung, so it is recorded as `unavailable_not_exposed`.

The deterministic local provider exercised three native CodeGG paths: repeated identical failed reads at unchanged state, the same read after a real workspace mutation, and repeated failures stopped by the explicit turn budget. ACP marked each actual failed read `failed`; error-like text on a typed-success projection remains successful. Injected observation gaps and missing state revisions remain `unknown`.

## C004 reconciliation

Recommendation: supersede only C004's forward-looking requirement that a repeated identical failure produce a terminal `stalled`. That terminal is incompatible with accepted ADR-0040 Option B and Recovery C002. The v2 control now enters the repeated-failure branch and records bounded completion or the independent turn-budget owner without inventing a stall terminal. Keep C004's original live sample and its historical `not_exercised` fact intact; do not rewrite its receipt, digest or closure narrative.

## Evidence

- Focused Python suite: 12 tests passed, including the native ACP repeated-failure, changed-state and turn-budget controls, v2 schema/tamper checks, unknown-gap behavior and v1-manifest immutability.
- Native binary source SHA-256 and per-call workspace revision digests are captured in deterministic run results; no provider network is contacted.
- Product correction in `src/agent/tool_batch.rs` preserves broker terminal status when handing results to the agent loop. ACP `ToolCompleted` now receives failure from the typed error path rather than output-text inference.
- The v2 evaluator stores digests and bounded status fields only; raw tool output is never included in the control receipt.

The exact resulting-main CI requirement remains H001-owned. H002 makes no claim that the integration branch is merged or that hosted CI passed on a merge SHA.
