# Decision-Model Extraction and Runtime Milestone 004 — System One Backend

Status: closing

Repository baseline: `2d0410b`

Source roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m004--system-one-compatible-backend`

Applicable ADRs:

- `plans/adrs/ADR-0013-external-decision-model-training-and-runtime-boundary.md`
- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: capability

Hard dependency:

- M001 closed.

Soft dependency:

- M003 may proceed independently. M004 exists partly to prove the CodeGG decision contract is not specific to the extracted local-artifact runtime.

## 1. Objective

Add an optional System One-compatible `DecisionEngine` backend that can evaluate CodeGG Binary, Choice, and Score requests through a configured `/v1/systemone` endpoint while preserving explicit capability mismatch for Rank/multi-relevance semantics, bounded deadlines, no-network-by-default behavior, and deterministic fallback.

The initial compatibility targets are the reference TypeSafe API shape and a separately identified Ollama-compatible profile. The implementation must not assume that every service advertising System One behavior accepts identical criteria forms or extensions.

## 2. Current external contract

At planning time (2026-10-05), the reference TypeSafe API exposes:

- `POST /v1/systemone`;
- a required model;
- shared `state`;
- one or more named questions;
- question families including choice, noul/yes-no, and score;
- `GET /v1/models` for model discovery.

Ollama 0.35 exposes a local `/v1/systemone` endpoint modeled on the same interface. Public issue evidence already demonstrates at least one criteria-schema difference from the reference implementation. Therefore compatibility is a validated profile/subset, not a boolean vendor claim.

These external facts are implementation inputs, not CodeGG's internal protocol.

## 3. Invariants that must not regress

- No System One endpoint is contacted unless the user explicitly configures/selects that backend.
- Remote TypeSafe/Jev credentials never enter logs, projections, model state, or training capture.
- Local Ollama still counts as an external process/network backend and is not auto-started by CodeGG.
- Rank with multiple independently relevant candidates is never silently mapped to exclusive Choice.
- Unsupported semantic kinds return Unsupported and fall back according to CodeGG policy.
- A timeout, 4xx/5xx, malformed response, unavailable model, protocol mismatch, or connection failure cannot fail the agent turn.
- Backend responses cannot widen the application-supplied candidate set or tool authority.
- The adapter does not create another retry/HTTP authority; it uses the repository's established outbound HTTP ownership, expected to be eggfetch-backed.
- Causal-frontier behavior remains independent.

## 4. Scope

### In scope

- Optional System One backend configuration.
- TypeSafe/reference and Ollama compatibility profiles over the common supported subset.
- Mapping:
  - Binary -> noul/yes-no;
  - Choice -> choice;
  - Score -> score.
- Explicit Rank Unsupported unless a future accepted contract defines a lossless mapping.
- Bounded request serialization and response validation.
- Optional model-list probing when explicitly requested/configured.
- Timeouts/cancellation.
- Credential injection for remote endpoints through existing secret/config mechanisms.
- Fake-server contract tests.
- Diagnostics and backend state.
- At least one operator-run compatibility smoke path per supported profile when credentials/service availability exist; lack of live remote credentials may not block correctness closure if the fake-server/reference-schema evidence is complete, but must be recorded honestly.

### Explicitly out of scope

- Hosted-service account provisioning.
- Starting/managing Ollama.
- Downloading decision models.
- Images/multimodal System One extensions.
- Turning Rank into repeated Choice calls.
- Default-on remote decisions.
- Automatic failover across arbitrary remote endpoints.
- New training/telemetry upload.

## 5. Required production changes

### Configuration

Add a backend configuration that separates:

- backend kind;
- base URL;
- model name;
- compatibility profile;
- credential reference/source when required;
- request timeout;
- optional model-discovery enablement.

Defaults must select no external backend.

URL parsing must reject unsupported schemes for remote credential-bearing use. Plain HTTP may be allowed only for explicitly local loopback endpoints if consistent with existing outbound policy; document the rule.

### HTTP ownership

Use the established CodeGG/eggfetch outbound path. Do not add reqwest or a bespoke retry client.

The decision backend owns one bounded request attempt unless the shared HTTP layer performs transparent transport behavior already accepted elsewhere. No unbounded retry/backoff loop belongs in request preparation.

### Request mapping

Build a System One request only from validated M001 decision types.

Question names should be stable generated ids independent of model-visible labels. Option labels/descriptions are escaped/bounded.

For each semantic family, document the exact mapping and response interpretation. Confidence fields are diagnostics; do not treat provider-specific confidence as calibrated probability of correctness unless the protocol defines that.

### Compatibility profiles

Define at least:

- reference profile;
- Ollama profile.

Profiles capture only actual differences CodeGG needs. Do not fork the whole schema unless necessary.

A profile must reject a request shape known not to be accepted rather than sending it and hoping.

### Response validation

Validate:

- model/answer envelope presence;
- exact requested question ids;
- answer type corresponds to question type;
- returned choice/levels belong to the request;
- probabilities/scores finite and bounded as declared;
- no extra answer can create a CodeGG candidate;
- response size bounds.

Malformed responses become backend failure, never partial actuation.

### Privacy projection

Remote backends receive only the M001 bounded state. Add a diagnostic mode that can report field names/byte counts without logging values.

## 6. Ordered work packages

### A — Protocol DTO/profile layer

Implement local serializable request/response DTOs and exact profile validation.

Acceptance: reference examples and known incompatibility fixtures distinguish accepted vs rejected shapes.

### B — HTTP backend and cancellation

Implement the `DecisionEngine` backend using established outbound HTTP.

Acceptance: deadlines/cancellation propagate and no request occurs while backend is off.

### C — Semantic mappings

Implement Binary/Choice/Score mappings.

Acceptance: fixture vectors preserve named question identity, option identity, probabilities/scores, and explicit abstention/unsupported behavior.

### D — Negative/failure matrix

Use a local fake server to cover timeout, connect failure, 401/403, 404, 422, 429, 5xx, malformed/truncated JSON, unknown answer ids, invalid probabilities, unsupported model, and schema/profile mismatch.

Acceptance: every case yields bounded backend status and deterministic fallback.

### E — Model discovery/operator smoke

Expose an explicit diagnostic/inspect path for `GET /v1/models` where supported, without background polling.

Acceptance: fake-server discovery passes; live smoke evidence is recorded when available and never fabricated.

### F — Documentation

Document endpoint profiles, privacy, credentials, limitations, and why Rank remains unsupported.

## 7. Failure, cancellation, restart, and contention

No durable backend state beyond configuration/circuit-breaker diagnostics is required.

A request is bounded by the caller's decision deadline. If the service exceeds it, cancel/drop and fall back.

Repeated failures may mark the backend degraded for a bounded process/session interval, but a recovery probe must not run in a background loop.

Configuration replacement is snapshot-based for in-flight turns.

## 8. Security

- Remote credentials use existing secret handling and are redacted.
- Remote endpoints use TLS except an explicitly local loopback allowance.
- Redirect behavior must follow existing provider/outbound policy and must not forward credentials to an untrusted origin.
- State payloads exclude secrets/raw tool args/results by default.
- Error bodies are bounded before logging and sanitized.
- Model-list discovery cannot mutate/install models.
- The response never supplies executable tool names outside the request candidates/options.

## 9. Compatibility and migration

No database migration.

Backend config is additive. Missing/unknown profile means unavailable/unsupported and falls back; startup remains usable.

The System One adapter version should be observable separately from the remote model version.

## 10. Required tests

- DTO serialization/deserialization;
- profile-specific criteria validation;
- Binary/Choice/Score round trips;
- Rank Unsupported;
- off -> zero network attempts;
- credential/redaction tests;
- redirect/host policy tests through existing HTTP abstractions;
- all failure statuses;
- response-id/candidate injection rejection;
- timeout/cancellation;
- concurrent config replacement snapshot behavior.

## 11. Required verification commands

Use the actual fake-server test target and feature names implemented. At minimum (omit `--all-features`: repository guidance prohibits broad workspace feature sweeps because that enables installed real-server tests):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --locked -p codegg --lib decision
cargo test --locked -p codegg --lib decision -- --nocapture  # unit target hosts the local fake server
scripts/verify.sh quick
git diff --check
```

Record a network-attempt spy proving default configuration is silent.

## 12. Documentation updates

- `architecture/tool-advisor.md`
- backend/config reference
- security/privacy docs if outbound decision state needs a new documented class
- roadmap/registry.

## 13. Acceptance criteria

M004 closes only when:

1. the backend is explicitly opt-in;
2. Binary/Choice/Score map losslessly over the supported profile subset;
3. Rank is explicit Unsupported;
4. fake-server negative/error coverage proves bounded fallback;
5. default/off mode makes zero System One network attempts;
6. credentials/state logging rules are proven;
7. candidate/authority widening is impossible;
8. compatibility profile/version is observable.

## 14. Stop conditions

Stop and write a corrective/new ADR if:

- live CodeGG behavior requires treating System One as the internal domain contract;
- Rank must be approximated with repeated/exclusive choice;
- credentials require a new ad-hoc secret store;
- the backend needs to auto-start or auto-install a service/model;
- response data can inject executable candidates;
- protocol differences cannot be bounded by explicit profiles.

## 15. Closure evidence required

- supported profile/schema table;
- fake-server result matrix;
- no-network-by-default evidence;
- security/redaction evidence;
- any live smoke evidence clearly labeled local/remote and optional;
- verification commands/results;
- unresolved findings;
- dependency audit for M005 interface stability.
