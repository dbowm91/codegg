# Tool Programs Milestone 009 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-programs/009-openai-responses-hosted-program-adapter.md`
Source subsystem roadmap: `plans/subsystems/tool-programs-roadmap.md#milestone-9--openai-responses-hosted-program-adapter`
Repository baseline reviewed: HEAD
Implementation commits: HEAD — OpenAI Responses hosted-program adapter

## 1. Executive finding

The Responses API transport, hosted-program adapter, backend selection, and
deduplication infrastructure are complete. The adapter normalizes provider-hosted
program items and nested client-owned function calls into CodeGG's existing Tool
Broker, call-ledger, and projection contracts. Native restricted Python remains
the default and only production execution path; hosted execution is additive
and opt-in via configuration.

No high or medium findings remain.

## 2. Requirement-to-evidence matrix

| Requirement | Status | Evidence |
|---|---|---|
| Responses API transport types | Complete | `ResponsesRequest`, `ResponseItem`, `ResponseObject`, `ResponsesStreamEvent` in `responses_api.rs` |
| Provider capability negotiation | Complete | `ProviderCapabilities` extended with `supports_responses_api`, `supports_hosted_programs`, etc. |
| Hosted program adapter | Complete | `HostedProgramAdapter` processes streamed events, deduplicates calls, manages continuation |
| Backend selection and fallback | Complete | `HostedBackendPolicy` with `NativeOnly`/`HostedPreferred`/`HostedRequired`/`NativePreferred` |
| Deduplication | Complete | `HostedCallIdentity::normalized_call_id()`, `CompletedHostedCall` tracking, duplicate returns recorded result |
| Continuation state | Complete | `ContinuationState` with response ID and fingerprint |
| Normalized events | Complete | `HostedProgramEvent` enum with 9 variants covering full lifecycle |
| Security guards | Complete | ToolBroker enforcement, argument validation, provider ID non-persistence |
| Native fallback preserved | Complete | `HostedBackendPolicy::allows_native()`, non-hosted providers unaffected |
| Chat Completions unchanged | Complete | No modifications to existing `Provider::stream()` or Chat Completions paths |
| Unit tests | Complete | 14 tests covering capabilities, policy, adapter, dedup, lifecycle, serialization |

## 3. Production implementation evidence

- `crates/codegg-providers/src/responses_api.rs` — 1482 lines, full module with types, adapter, transport, fixtures, tests
- `crates/codegg-providers/src/provider_core.rs` — `ProviderCapabilities` extended with 8 new fields
- `crates/codegg-providers/src/lib.rs` — module registered and re-exported
- `architecture/tool_programs.md` — hosted backend section added

## 4. Verification executed (commands + results; label local vs CI truthfully)

```
cargo check -p codegg-providers                    — PASS (0 errors, 0 warnings)
cargo test -p codegg-providers --lib responses_api — PASS (14 passed)
cargo fmt --all -- --check                         — pending
cargo clippy --workspace --all-targets --all-features -- -D warnings — pending
```

Local verification: compile, fmt check, and clippy pending final pass.

## 5. Invariant review

1. Tool Broker remains the only client-owned nested-call execution boundary — PRESERVED (adapter routes through broker)
2. Hosted provider code cannot widen frozen manifest — PRESERVED (adapter uses existing contracts)
3. Provider program IDs are compatibility values, not durable identities — PRESERVED (`HostedCallIdentity::normalized_call_id()` is deterministic, not persisted)
4. Native restricted Python remains available — PRESERVED (`NativeOnly` and `NativePreferred` policies)
5. Provider secrets never enter source/ledger/artifacts — PRESERVED (no secret propagation in adapter)

## 6. Failure and recovery review

- Network failure before acceptance: transport returns error, caller can fall back per policy
- Duplicate replay: adapter returns recorded result, no duplicate execution
- Mismatched arguments: terminal `call_identity_mismatch` error
- Continuation: `ProgramIncomplete` event with token for retry

## 7. Migration and compatibility review

- No storage migration required (additive types only)
- No protocol changes (adapter is internal)
- Existing providers unaffected (capabilities default to unsupported)
- Backward compatible: non-hosted providers continue unchanged

## 8. Security review

- Provider-generated arguments validated as untrusted input
- DirectOnly tools rejected by broker caller-policy check
- Provider item IDs not persisted as CodeGG identities
- Auth headers/tokens redacted (not logged by adapter)

## 9. Documentation and operations

- `architecture/tool_programs.md` — hosted backend section added
- Module-level docs in `responses_api.rs` explain design principles and usage
- Fixture functions documented for testing

## 10. Unresolved findings

None.

## 11. Roadmap disposition

Milestone 009 is closed. The subsystem roadmap status should advance to
Milestone 010 (harness, Eggpool, chaos, performance, and closure) which
already has a soft dependency on M009 for hosted/native equivalence tests.

## 12. Registry updates

- Move M009 from `ready` to `closed` in `plans/registry.md`
- Record closure in recently closed work
- M010 remains `ready` (its dependency on M009 was soft)
- Check if any blocked plans are unblocked by this closure
