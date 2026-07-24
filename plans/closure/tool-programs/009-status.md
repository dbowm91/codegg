# Tool Programs Milestone 009 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-programs/009-openai-responses-hosted-program-adapter.md`
Source subsystem roadmap: `plans/subsystems/tool-programs-roadmap.md#milestone-9--openai-responses-hosted-program-adapter`
Repository baseline reviewed: HEAD
Implementation commits: HEAD — OpenAI Responses hosted-program adapter (full implementation)

## 1. Executive finding

The Responses API transport, hosted-program adapter with full broker
integration, backend selection, deduplication, security guards, and
documentation are complete. The adapter normalizes provider-hosted
program items and nested client-owned function calls into CodeGG's
existing Tool Broker, call-ledger, and projection contracts. Native
restricted Python remains the default and only production execution
path; hosted execution is additive and opt-in via configuration.

No high or medium findings remain.

## 2. Requirement-to-evidence matrix

| Requirement | Status | Evidence |
|---|---|---|
| Responses API wire types | Complete | `ResponsesRequest`, `ResponseItem`, `ResponseObject`, `ResponsesStreamEvent` in `responses_api.rs` |
| Provider capability negotiation | Complete | `ProviderCapabilities` extended with 12 hosted program fields including `requires_fingerprint`, `supports_output_schema`, `max_result_size`, `max_tool_calls_per_program` |
| Hosted program adapter | Complete | `HostedProgramAdapter` implements 8-step broker integration pipeline |
| Backend selection and fallback | Complete | `HostedBackendPolicy` with `NativeOnly`/`HostedPreferred`/`HostedRequired`/`NativePreferred` |
| Deduplication | Complete | `HostedCallIdentity::normalized_call_id()`, `CompletedHostedCall` tracking, duplicate returns recorded result |
| Continuation state | Complete | `ContinuationState` with response ID and fingerprint |
| Normalized events | Complete | `HostedProgramEvent` enum with 9 variants covering full lifecycle |
| Broker step 3: tool validation | Complete | `validate_tool_call()` checks deny/allow lists, client-owned capability, argument structure |
| Broker step 4: ledger reservation | Complete | `reserve_call()` with call count limit enforcement, `ReservedCall` tracking |
| Broker step 6: result validation | Complete | `record_call_result()` validates result size, removes reservation, stores completed call |
| Broker step 7: bounded output | Complete | `build_call_output()` truncates oversized results |
| Broker step 8: continuation persistence | Complete | `ContinuationState` updated on `ResponseCompleted` and `Incomplete` events |
| Transport cancellation | Complete | `ResponsesTransport::cancel()` with `AtomicBool` flag, stream checks cancellation at each iteration |
| Transport timeout | Complete | `ResponsesTransportConfig` with `request_timeout`, `stream_idle_timeout`, `max_sse_buffer_size` |
| Security: argument validation | Complete | `validate_arguments()` checks JSON validity, object type, size bounds |
| Security: result size bounds | Complete | `validate_result_size()` enforces per-call payload limits |
| Security: call count bounds | Complete | `validate_call_count()` enforces nested call limits |
| Security: body minimization | Complete | `minimize_input_items()` truncates large FunctionCallOutput for provider transmission |
| Security: artifact filtering | Complete | `filter_artifacts_for_provider()` only sends selected artifacts |
| Security: redaction | Complete | `redact_for_log()`, `redact_fingerprint()` mask sensitive values |
| Native fallback preserved | Complete | `HostedBackendPolicy::allows_native()`, non-hosted providers unaffected |
| Chat Completions unchanged | Complete | No modifications to existing `Provider::stream()` or Chat Completions paths |
| Unit tests | Complete | 35 tests covering capabilities, policy, adapter, dedup, lifecycle, validation, redaction, transport |
| Integration tests: adapter | Complete | `hosted_tool_program_adapter.rs` — 28 tests covering full lifecycle, dedup, broker steps, minimization, filtering |
| Integration tests: recovery | Complete | `hosted_tool_program_recovery.rs` — 8 tests covering restart scenarios, reservation release, continuation preservation |
| Integration tests: security | Complete | `hosted_tool_program_security.rs` — 29 tests covering tool rejection, argument/result bounds, redaction, cross-program isolation |
| Integration tests: contention | Complete | `hosted_tool_program_contention.rs` — 21 tests covering cancel during stream/nested call/terminal, many programs, idle/timeout/backpressure, rapid operations |
| Integration tests: equivalence | Complete | `hosted_tool_program_equivalence.rs` — 8 tests covering native vs hosted event equivalence, call counts, dedup, continuation, error handling, mixed tools |

## 3. Production implementation evidence

- `crates/codegg-providers/src/responses_api.rs` — Full module with wire types, adapter (8-step broker integration), transport (cancellation, timeout, stream-idle), security validators, redaction helpers, fixture builders
- `crates/codegg-providers/src/provider_core.rs` — `ProviderCapabilities` extended with 12 hosted program fields
- `crates/codegg-providers/src/lib.rs` — Module registered and re-exported with all public types
- `architecture/tool_programs.md` — Hosted backend section with lifecycle, configuration, troubleshooting, privacy/data-flow, and fallback policy documentation
- `tests/hosted_tool_program_adapter.rs` — Integration tests for full lifecycle, dedup, broker steps, body minimization, artifact filtering
- `tests/hosted_tool_program_recovery.rs` — Integration tests for restart/recovery scenarios
- `tests/hosted_tool_program_security.rs` — Integration tests for security guards
- `tests/hosted_tool_program_contention.rs` — Integration tests for contention, cancellation, limits, backpressure
- `tests/hosted_tool_program_equivalence.rs` — Integration tests for native vs hosted equivalence fixtures

## 4. Verification executed (commands + results)

```
cargo check -p codegg-providers                          — PASS (0 errors)
cargo test -p codegg-providers --lib responses_api       — PASS (35 passed)
cargo test --test hosted_tool_program_adapter            — PASS (28 passed)
cargo test --test hosted_tool_program_recovery           — PASS (8 passed)
cargo test --test hosted_tool_program_security           — PASS (29 passed)
cargo test --test hosted_tool_program_contention         — PASS (21 passed)
cargo test --test hosted_tool_program_equivalence        — PASS (8 passed)
cargo fmt --all -- --check                               — PASS (clean)
cargo clippy -p codegg-providers -- -D warnings          — PASS (no issues)
```

Total: 129 tests (35 unit + 94 integration)

## 5. Invariant review

1. Tool Broker remains the only client-owned nested-call execution boundary — PRESERVED (adapter validates via `validate_tool_call()` and routes through broker)
2. Hosted provider code cannot widen frozen manifest — PRESERVED (adapter uses existing contracts, denied tools list enforced)
3. Provider program IDs are compatibility values, not durable identities — PRESERVED (`HostedCallIdentity::normalized_call_id()` is deterministic, not persisted as CodeGG identity)
4. Native restricted Python remains available — PRESERVED (`NativeOnly` and `NativePreferred` policies)
5. Provider secrets never enter source/ledger/artifacts — PRESERVED (no secret propagation, redaction helpers)

## 6. Failure and recovery review

- Network failure before acceptance: transport returns error, caller can fall back per policy
- Duplicate replay: adapter returns recorded result, no duplicate execution (tested in 3 integration tests)
- Mismatched arguments: terminal `call_identity_mismatch` error (tested)
- Continuation: `ProgramIncomplete` event with token for retry (tested)
- Restart before first item: adapter starts clean (tested)
- Restart during nested call: reservation released, no state leak (tested)
- Restart after result: completed calls preserved, replay returns recorded result (tested)
- Mismatched args after restart: terminal error (tested)

## 7. Cancellation and contention review

- Cancel during stream: transport `cancel()` sets flag, stream terminates on next poll (tested)
- Cancel during nested call: `release_reservation()` removes in-flight call, no state leak (tested)
- Cancel during terminal publication: cancel after terminal event is ignored, state preserved (tested)
- Cancel with error event: completed calls preserved after error (tested)
- Many programs (50 adapters): independent state, cross-adapter isolation verified
- Many calls within limits: `max_nested_calls` enforced, reservation and completion counts correct
- Rapid reserve/release cycles: 100 iterations, no state corruption
- Interleaved operations stress: reserve, complete, release interleaving verified
- Backpressure via result size: `max_result_size` enforced, oversized results rejected
- Backpressure via call count: limits enforced, release frees slots

## 8. Native vs hosted equivalence review

- Single read: both paths produce `ProgramStarted`, `NestedCall`, `Terminal`, `Usage` events
- Multi-call program: both paths complete same call counts, same tool names
- Deduplication: both paths return recorded result on duplicate, no duplicate execution
- Continuation state: both paths preserve same `response_id` after `ResponseCreated`
- Error handling: both paths emit `Error` events with same code/message
- Incomplete/continuation: both paths emit `ProgramIncomplete` with same reason and token
- Mixed tool types: both paths normalize read + grep calls equivalently
- Backend resolution: `NativeOnly` and `HostedPreferred` produce same event types

## 9. Migration and compatibility review

- No storage migration required (additive types only)
- No protocol changes (adapter is internal)
- Existing providers unaffected (capabilities default to unsupported)
- Backward compatible: non-hosted providers continue unchanged

## 10. Security review

- Provider-generated arguments validated as untrusted input (JSON structure, object type, size bounds)
- DirectOnly tools rejected by adapter's denied tools list and validate_tool_call()
- Provider item IDs not persisted as CodeGG identities
- Auth headers/tokens redacted (not logged by adapter)
- Argument size bounds enforced (MAX_ARGUMENT_SIZE = 1 MB)
- Result size bounds enforced (per-call and per-program limits)
- Call count bounds enforced (MAX_NESTED_CALLS = 100)
- Body minimization limits data sent to provider on continuation
- Artifact filtering prevents sending unselected artifacts to provider
- Cross-program isolation: different program IDs produce different normalized call IDs

## 11. Documentation and operations

- `architecture/tool_programs.md` — Hosted backend section with lifecycle, configuration, troubleshooting, privacy/data-flow, and fallback policy
- Module-level docs in `responses_api.rs` explain design principles
- Fixture functions documented for testing

## 12. Unresolved findings

None.

## 13. Roadmap disposition

Milestone 009 is closed. The subsystem roadmap status should advance to
Milestone 010 (harness, Eggpool, chaos, performance, and closure) which
already has a soft dependency on M009 for hosted/native equivalence tests.

## 14. Registry updates

- M009 remains `closed` in `plans/registry.md`
- M010 remains `ready` (its dependency on M009 was soft, now satisfied)
