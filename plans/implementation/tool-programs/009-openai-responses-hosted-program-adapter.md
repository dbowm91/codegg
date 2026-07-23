# Tool Programs Milestone 009 — OpenAI Responses Hosted-Program Adapter

Status: blocked pending Milestone 008 closure and stable provider capability interface

Repository baseline: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (`main`)

Source roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-9--openai-responses-hosted-program-adapter`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/001-terminology-and-domain-model.md` — provider connection, job, run, artifact, execution context

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: capability / interoperability

## 1. Objective

Add an optional OpenAI Responses API transport and hosted-program adapter that normalizes provider-hosted program source, nested client-owned calls, caller lineage, continuation/fingerprints, incomplete state, and program output into CodeGG’s existing Tool Program, Tool Broker, scheduler, call-ledger, artifact, cancellation, and projection contracts.

Hosted execution is an optimization/backend choice. It must not become a second policy or persistence architecture.

## 2. Readiness boundary

Hard dependency: M008 closure. The native runtime, background delivery, projections, and exactly-once parent notification must be stable first.

Interface dependency: provider capability negotiation must be able to represent Responses support, hosted program support, supported languages/features, continuation requirements, and fallback behavior.

## 3. Current implementation evidence

- The OpenAI-compatible provider serializes Chat Completions messages and ordinary function tools.
- `ToolDefinition` maps to ordinary OpenAI/Anthropic function schemas and does not represent output schemas or allowed callers on the provider wire.
- Provider events model text, reasoning, normal tool calls/results, finish, and errors; no Responses item graph, caller lineage, program output, fingerprint, or incomplete continuation state exists.
- The Tool Program domain already owns native program source, manifests, nested calls, results, background delivery, and artifacts after M008.
- OpenAI’s programmatic calling design uses hosted program items and nested client-owned function calls, requiring continuation of the correct response/program lineage.

## 4. Invariants that must not regress

- Tool Broker remains the only client-owned nested-call execution boundary.
- Hosted provider code cannot widen the frozen manifest or call direct-only tools.
- Scheduler child-job policy, authorization, path checks, artifacts, cancellation, and output schema validation remain native.
- Provider program IDs/fingerprints are compatibility/provenance values, not CodeGG durable identities.
- Provider secrets and raw credentials never enter source, ledger, artifacts, projections, or diagnostics.
- Native restricted Python remains available for providers without hosted support and as a configured fallback.
- Fallback never silently changes side-effect policy, result schema, or authority.
- Streaming item reordering, duplication, retry, or reconnect cannot duplicate nested calls.
- Provider hidden reasoning is not persisted or exposed as program state.

## 5. Scope

### In scope

- Separate Responses API transport/client path where required.
- Capability discovery/configuration and explicit backend selection.
- Provider-neutral normalized events for program item, nested function call, program output, incomplete state, caller, fingerprint, and terminal usage.
- Mapping hosted nested calls to `ProgramCallId` and Tool Broker.
- Continuation/retry with preserved response/program lineage.
- Background and foreground hosted-program handling through existing notification/projection paths.
- Native fallback and deterministic fixture testing.
- Documentation of unsupported OpenAI-specific features.

### Explicitly out of scope

- Replacing all OpenAI-compatible Chat Completions traffic with Responses.
- Local JavaScript execution.
- Bypassing CodeGG scheduler with provider-hosted shell or computer tools.
- Enabling mutation/direct-only tools through hosted programs.
- Depending on a live OpenAI account for deterministic closure.
- General provider architecture rewrite unrelated to Responses item handling.

## 6. Required production changes

### Provider capability model

Extend provider/model capabilities with explicit fields such as:

- responses API supported;
- hosted programmatic tool calling supported;
- hosted language/version;
- client-owned nested function calls supported;
- background/continuation support;
- required response/item identifiers or fingerprints;
- output-schema/strict-function behavior;
- maximum program/tool/result limits where advertised.

Capabilities must come from trusted configuration/discovery and be snapshotted for an in-flight program. Unknown or contradictory capability fails closed.

### Responses transport

Implement a separate transport abstraction if the current `Provider::stream(ChatRequest)` cannot represent the item graph without semantic loss. It must support:

- request creation with ordinary client-owned tool contracts and hosted-program eligibility;
- streaming item lifecycle and stable item IDs;
- nested function-call arguments as bounded structured data;
- submitting nested tool results to continue the correct response/program;
- response/program fingerprints or continuation tokens;
- cancellation and network timeout;
- terminal usage and incomplete/error details;
- bounded buffering and malformed stream handling.

Do not force Responses semantics into Chat Completions message arrays if that loses caller/item identity.

### Normalized provider events

Add internal provider-neutral variants, for example:

- `ProgramStarted` with provider item identity and source metadata;
- `ProgramNestedCall` with caller identity, call ID, tool, arguments, and sequence;
- `ProgramOutput` with structured final value;
- `ProgramIncomplete` with reason/continuation data;
- `ProgramFingerprint` or opaque continuation state;
- terminal/error/usage.

Persist only bounded required provider state. Treat provider source as untrusted and validate language/manifest/limits before associating it with a CodeGG program.

### Broker and ledger integration

For each hosted nested call:

1. resolve or create the deterministic CodeGG `ProgramCallId` from program/provider item/call identity;
2. reject duplicate mismatched identities;
3. validate tool contract/caller policy/arguments/authority through Tool Broker;
4. reserve the call ledger before execution;
5. execute inline or scheduler child job normally;
6. validate/persist result and artifacts;
7. return the bounded provider-facing tool result to the correct hosted program;
8. persist continuation state before waiting for more items.

If a provider repeats a call after transport retry, return the recorded result rather than executing again.

### Backend selection and fallback

Support explicit policies:

- native only;
- hosted preferred with native fallback before execution begins;
- hosted required;
- native preferred.

Fallback is allowed only before any hosted nested call or provider-owned state makes native replay semantically ambiguous. Mid-program fallback is prohibited unless an explicit portable checkpoint conversion exists, which is outside this milestone.

### Security and data handling

- Minimize source/tool-result bodies sent to provider and honor configured retention/privacy policy.
- Never send artifacts or files not explicitly selected by program calls.
- Validate provider-generated arguments as untrusted model output.
- Redact auth headers, response tokens, fingerprints, and provider IDs where required in logs/events.
- Add request-size, item-count, nested-call, output, and stream-idle bounds.

## 7. Ordered work packages

### Work package A — Capability and transport boundary

- Add provider/model capability types and backend policy.
- Implement Responses request/stream fixture transport separately from Chat Completions where necessary.
- Add bounded item parser and diagnostics.

### Work package B — Normalized program event model

- Add provider-neutral program events and opaque continuation/fingerprint storage.
- Map provider item identities to CodeGG program/call records.
- Preserve streaming order and duplicate detection.

### Work package C — Nested Tool Broker loop

- Reserve/execute/record/respond for client-owned calls.
- Reuse completed call results on repeated provider items.
- Route heavy calls through scheduler child jobs.
- Propagate cancellation and deadlines.

### Work package D — Background, continuation, and fallback

- Integrate hosted terminal/incomplete state with M008 projections/notifications.
- Implement continuation across reconnect/retry.
- Enforce pre-execution-only fallback and explicit required/preferred policies.

### Work package E — Fixtures, privacy, docs, and guards

- Add recorded/synthetic item-stream fixtures for all lifecycle paths.
- Add malformed, reordered, duplicated, truncated, and delayed stream tests.
- Add guards that hosted calls cannot invoke Tool implementations directly.
- Document capability and configuration behavior.

## 8. Failure, cancellation, restart, and contention semantics

- Network/provider failure before program acceptance may retry or fall back according to policy.
- Failure after hosted state exists resumes with persisted response/program lineage; it does not create a new logical program.
- Duplicate/replayed nested-call item returns the ledger result if identity and normalized arguments match; mismatch is terminal provider-protocol failure.
- Cancellation closes provider stream/request, cancels active broker calls/child jobs, persists hosted state, and publishes one terminal result.
- Stream idle timeout and program stall timeout are distinct but both bounded.
- Restart reloads provider continuation state and completed call ledger, then resumes or marks recoverable if the provider cannot continue.
- Rate limit/service unavailable uses bounded Retry-After-aware backoff; auth/model/schema errors do not retry blindly.
- Provider concurrency remains subject to connection/model limits and program/scheduler budgets.

## 9. Compatibility and migration

- Existing OpenAI-compatible Chat Completions provider remains available and unchanged for normal function calling.
- Responses support is additive and negotiated/configured.
- Eggpool and other compatible providers use native restricted Python unless they explicitly implement the required Responses semantics.
- Older clients see normalized generic program/job projections.
- Opaque provider continuation state is versioned and may become recoverable/blocked if an adapter version is removed.

## 10. Required tests

### Focused unit tests

- capability negotiation and backend selection;
- Responses request serialization and item parsing;
- caller/fingerprint/continuation normalization;
- duplicate call identity and argument-match rules;
- fallback eligibility.

### Integration tests

- hosted foreground/background program with nested read and build/test calls through production Tool Broker;
- exact program/call/job/run/artifact correlation;
- terminal/incomplete notification integration;
- native versus hosted normalized result equivalence fixtures.

### Restart and recovery tests

- restart before first item, after program start, during nested call, after result before provider continuation, and before terminal notification;
- repeated item returns recorded result without duplicate execution;
- unavailable continuation yields recoverable terminal state.

### Contention and cancellation tests

- cancel during stream, nested inline call, child job, backoff, and terminal publication;
- many hosted programs respect provider and scheduler limits;
- idle/timeout/backpressure convergence.

### Security and negative tests

- provider asks for direct-only/mutating tool;
- malformed/oversized arguments or source;
- forged caller/fingerprint/call IDs;
- cross-program result injection;
- secret/header/token leakage;
- item reordering/duplication/truncation.

## 11. Required verification commands

```bash
cargo test -p codegg-providers --lib responses
cargo test -p codegg --test hosted_tool_program_adapter
cargo test -p codegg --test hosted_tool_program_recovery
cargo test -p codegg --test hosted_tool_program_security
cargo test -p codegg --test tool_program_notifications
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

Live-provider evidence, if run, is supplemental and must identify model, endpoint class, date, configuration, and redaction. Deterministic closure rests on fixtures and local policy tests.

## 12. Documentation updates

- provider architecture and capability negotiation
- `architecture/tool_programs.md` hosted backend section
- OpenAI Responses configuration and troubleshooting
- privacy/data-flow documentation
- native/hosted fallback policy

## 13. Acceptance criteria

1. Hosted nested calls execute only through the native Tool Broker and scheduler.
2. Provider item/caller/fingerprint/continuation state is preserved without becoming CodeGG identity authority.
3. Duplicate provider items never duplicate nested effects.
4. Cancellation/restart/retry converge with one program and one terminal notification.
5. Native fallback is explicit, pre-execution-only, and semantically safe.
6. Chat Completions and non-hosted providers remain operational.
7. Deterministic native/hosted shared fixtures produce equivalent normalized results and evidence.
8. No unresolved high or medium finding remains.

## 14. Stop conditions

Stop and report if M008 is not closed, current Provider abstractions cannot represent Responses without identity loss and a separate transport is disallowed, hosted continuation cannot be persisted safely, or provider behavior would require bypassing broker/scheduler policy or enabling mutation tools.

## 15. Closure evidence required

Create `plans/closure/tool-programs/009-status.md` with capability/transport decision, item mapping matrix, duplicate/restart execution counts, native/hosted equivalence, cancellation/backpressure results, privacy/redaction evidence, compatibility results, live evidence clearly separated from deterministic tests, and residual findings.

## 16. Handoff notes

Use official API fixtures/specification as the wire reference, but keep internal types provider-neutral. Do not encode OpenAI response IDs into durable typed identity newtypes. Do not require this adapter for Eggpool or general Tool Program availability.
