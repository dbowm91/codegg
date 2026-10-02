# Provider Wire-Kernel Consolidation Milestone 001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/provider-wire-kernel-consolidation/001-shared-wire-bridge-and-parity-qualification.md`

Source subsystem roadmap: `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md#milestone-001--shared-wire-bridge-and-parity-qualification`

Repository baseline reviewed: `53dea47f` (latest `origin/main` before this branch)

Implementation commits:

- `94a4b3a0` — qualify the shared provider wire bridge and model-policy projection
- `74ded2b8` — move M001 into closure review

## 1. Executive finding

M001 is complete. CodeGG now pins the neutral `eggpool-wire` package at
`f05b18b7358d9a4125d1e20c491151eec265e403`, converts its semantic provider
requests through one typed bridge, and consumes canonical streams through the
upstream incremental decoder and bounded completed-tool-call accumulator.
Existing provider request builders and parsers remain in production use;
M001 did not cut over a provider family.

The shared bridge preserves CodeGG's existing OpenAI Chat `max_tokens`
contract with a closed application-layer correction. Resolved model-adapter
facts now travel in a conservative immutable `ProviderWirePolicy`; the
OpenAI-compatible provider no longer recognizes Laguna policy by matching
model names.

No unresolved medium-or-higher finding remains. Local required verification
passed on the installed Rust 1.89 arm64 toolchain. CI was not run.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Immutable neutral dependency; no EggPool runtime stack | Cargo.lock pins the exact M006 commit. `scripts/check_provider_wire_boundary.py` and `cargo tree -p codegg-providers --edges normal` pass; the provider tree contains `eggpool-wire` and no EggPool runtime/routing packages. | Pass |
| Typed semantic request bridge | `crates/codegg-providers/src/wire.rs` converts every CodeGG message/content variant, interrupted tool history, tools, response formats, controls, thinking intent, and request options without JSON round-trips. Focused bridge tests pass. | Pass |
| Conservative private reasoning and one policy authority | Missing policy omits private reasoning and aliases. The turn runtime projects declarative adapter aliases, argument aliases, closed reasoning transforms, tool-choice, and parallel-tool controls. Provider-local Laguna model matching was removed. Provider transcript tests cover opt-in Laguna round-trip and conservative default behavior. | Pass |
| Shared stream framing and bounded tool accumulation | `SharedStreamDecoder` wraps the upstream SSE decoder and `CanonicalToolCallAccumulator`. Upstream successful vectors for all standard streaming families pass; OpenAI chunk-boundary replay is deterministic; interleaved complete calls normalize correctly. | Pass |
| Failure, EOF, and resource behavior | Malformed completed arguments fail closed; provider errors, EOF without completion, post-terminal payload, and incomplete decoder summaries return provider errors. Accumulator limits and state are upstream-owned and per-stream. | Pass |
| Provider parity matrix | See below. Differences that affect field names or provider semantics are explicit bridge corrections or remain owned by later provider cutover tests. | Pass for M001; M002 owns production-path qualification |
| No production provider cutover | OpenAI, OpenAI-compatible, Anthropic, Gemini, Bedrock, and hosted Responses production serializers/decoders remain on their existing paths. | Pass |

### Family matrix

| Family | Qualified behavior or classified difference | M001 disposition |
|---|---|---|
| OpenAI Chat | Basic request transcript is identical after the CodeGG bridge renames canonical `max_completion_tokens` to legacy `max_tokens`. `stream_options.include_usage` is explicit and opt-in at the shared API; CodeGG provider policy selects it. | Ready for M002 production-path qualification |
| Generic OpenAI-compatible | Uses the OpenAI Chat surface. Missing policy defaults to no private reasoning, no aliases, and no inferred model behavior. Optional stream usage remains caller-controlled. Provider-specific omission and transport behavior remain wrapper-owned. | Ready for M002 provider-by-provider qualification |
| Anthropic Messages | The shared codec moves a canonical system message to the top-level `system` field exactly once and leaves the user message in `messages`. | Ready for M003 production-path qualification |
| MiniMax Anthropic-compatible | Shares the Messages codec grammar but keeps its current endpoint and credential contract outside the bridge. | Ready for M003 production-path qualification |
| Google Gemini GenerateContent | Uses the native Gemini surface, not OpenAI syntax. Canonical system and image content remain represented in the generated request. Stream decoder success vectors pass. | Ready for M003 production-path qualification |
| Bedrock / hosted Responses | No M001 migration or codec claim. Their specialized paths remain unchanged. | Explicitly excluded |

## 3. Production implementation evidence

- `crates/codegg-providers/src/wire.rs` owns semantic request conversion,
  finite surface encoding, canonical event mapping, usage conversion, and
  per-stream decode state.
- `ProviderRequestContext.wire_policy` is immutable per request, optional,
  conservative by default, and contains no credentials, endpoint, or header
  map. The turn runtime projects it from the already-resolved CodeGG adapter.
- `openai_compatible.rs` consumes that projection and no longer has a
  Laguna regex/name table. Existing request serialization, SSE parsing,
  transport, and provider identity remain unchanged.
- The application bridge keeps the existing OpenAI `max_tokens` spelling
  while the upstream canonical codec remains provider-neutral.
- `scripts/check_provider_wire_boundary.py` is part of `scripts/verify.sh
  quick` and prevents runtime/routing packages entering the provider crate's
  normal dependency tree.

## 4. Verification executed

All commands below were executed locally. The test and Clippy commands used
Rust 1.89 arm64 because the default x86_64 Homebrew toolchain could not link
against the arm64 MacPorts libraries.

| Command | Result |
|---|---|
| `rtk cargo fmt --all -- --check` | Pass |
| `rtk python3 scripts/check_provider_wire_boundary.py` | Pass |
| `rtk cargo tree -p codegg-providers --edges normal` | Pass; no EggPool runtime/routing package appears |
| Rust 1.89 arm64 `cargo test -p codegg-providers` | Pass; 174 unit tests, 0 failed; doc-tests 0 |
| Rust 1.89 arm64 `cargo test --test provider_transcripts` | Pass; 21 tests, 0 failed |
| Rust 1.89 arm64 `cargo clippy -p codegg-providers --all-targets -- -D warnings` | Pass |
| `rtk cargo check --workspace` (local default toolchain) | Pass at implementation revision; the Rust 1.89 arm64 provider tests also compiled the changed provider crate and linked the root integration target |

No hosted CI result is claimed.

## 5. Invariant review

- `codegg-providers` does not depend on `codegg-core`.
- The shared dependency is pinned to EggPool's M006 closure revision.
- No source surface is fabricated for semantic requests.
- Private reasoning is omitted unless the resolved policy explicitly permits
  round-trip.
- Wire aliases reverse to canonical names before tool authorization.
- Credentials, URLs, session IDs, prompts, tool arguments, and provider bodies
  are not added to bridge diagnostics.
- Provider transport, endpoint/auth/header policy, deadlines, cancellation,
  retries, and durable identity remain CodeGG-owned.
- Bedrock SigV4/ConverseStream and hosted/stateful Responses remain specialized.

## 6. Failure and recovery review

The bridge is pure and request-scoped. Stream decoder and accumulator state is
owned by one `SharedStreamDecoder` and is dropped with the provider stream.
Malformed framing, malformed completed arguments, provider errors, incomplete
responses, missing terminal completion, and data after terminal completion
fail rather than becoming successful EOF. M001 does not alter retry decisions.

## 7. Migration and compatibility review

There is no storage or configuration migration. Provider IDs, endpoints,
credential modes, and durable connection records are unchanged. The new crate
is a Git dependency pinned to the full M006 closure commit; no publication or
semver-stability claim is introduced. The `max_tokens` bridge correction
preserves existing CodeGG wire behavior until M002 qualifies production
provider paths.

## 8. Security review

The kernel and bridge receive no credentials, URL, static headers, or session
identity. Provider errors from bridge failures use fixed redacted messages.
Private reasoning stays absent under the default policy. Canonical aliases are
reversed before authorization. The kernel's bounded accumulator rejects
over-limit and malformed state.

## 9. Documentation and operations

`architecture/provider.md` describes the bridge and compatibility transform.
`architecture/model-adapters.md` records the single resolved policy
projection. The dependency boundary guard runs in quick verification.

## 10. Unresolved findings

None. Severity: none.

## 11. Roadmap disposition

- Close M001.
- M002 is now dependency-ready: its sole hard/interface dependency, positive
  M001 closure and provider-family parity matrix, is satisfied. M002 begins
  production OpenAI-family qualification and preserves M001's wrapper and
  ownership boundaries.
- M003 remains blocked on M002 closure.
- No corrective pass is required. Provider-profile metadata deduplication
  remains intentionally outside this roadmap.

## 12. Registry updates

- Move M001 from closure review to recently closed with implementation
  reference `94a4b3a0` and this closure record.
- Move M002 from blocked to ready in the same closure commit. Dependency audit:
  M001 is its only hard dependency and is now closed; its required shared
  bridge and parity matrix are stable.
- Leave M003 blocked on M002.
- No other registered plan lists M001 as a dependency.
