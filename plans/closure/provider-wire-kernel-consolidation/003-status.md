# Provider Wire-Kernel Consolidation Milestone 003 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/provider-wire-kernel-consolidation/003-anthropic-gemini-cutover-and-wire-retirement.md`
Source subsystem roadmap: `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md#milestone-003--anthropicgemini-cutover-and-duplicate-wire-retirement`
Repository baseline reviewed: `2c6e6b9`
Implementation commits: `524cecf — cut over Anthropic and Gemini shared wire; 2c6e6b9 — move M003 to closure`

## 1. Executive finding

Anthropic Messages and Gemini GenerateContent production providers now use the pinned shared wire encoder and family-specific SSE decoder. The standard OpenAI, Anthropic, and Gemini duplicate parser/serializer paths are retired from CodeGG. CodeGG continues to own provider transport, endpoint/auth configuration, HTTP failures, and policy projection. The only known broad-suite failures are scheduler cancellation tests reproduced on untouched `origin/main`; provider-focused verification is green.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Anthropic family uses shared Messages encode/decode | `anthropic.rs` delegates to `WireSurface::AnthropicMessages` and `AnthropicMessagesSse`; transcript fixtures cover assistant text, tool calls/results, system and tools | Pass |
| Gemini uses native GenerateContent encode/decode | `google.rs` delegates to `WireSurface::GeminiGenerateContent` and `GeminiGenerateContentSse`; transcript fixture covers content, tools, generation config | Pass |
| Shared semantic bridge retains tool result identity | Canonical `ToolResult` carries its originating call ID; provider transcript cases pass | Pass |
| Reasoning history/output remains policy-gated | Canonical request projection filters private history; shared family stream bridge filters reasoning output absent explicit policy | Pass |
| Retire duplicate standard parser/serializer owners | Removed `sse_parser.rs` and Google-local parser; `check_provider_wire_cutover.py` guards OpenAI, Anthropic, and Gemini ownership | Pass |
| Preserve transport/auth/endpoints and specialized providers | Provider wrappers still own URLs, keys, headers, HTTP status behavior; Bedrock and Responses files unchanged | Pass |

## 3. Production implementation evidence

The Anthropic provider and MiniMax's Anthropic-compatible constructor share the Messages request/stream bridge while retaining their CodeGG endpoint and credential contracts. Google keeps its `streamGenerateContent` URL and `x-goog-api-key` header and routes only the body and event grammar through the shared Gemini surface. The Anthropic compatibility correction retains typed text content blocks. Tool results are represented as canonical tool-result blocks instead of plain text. Encoding failures fail before network submission in production paths.

## 4. Verification executed

All results are local, using Rust 1.89 on `aarch64-apple-darwin`:

| Command / evidence | Result |
|---|---|
| `cargo test -p codegg-providers --locked` | 174 passed |
| `cargo test --test provider_transcripts --locked` | 22 passed |
| `wire::tests::successful_upstream_family_vectors_decode_through_codegg_bridge` | Pass; upstream success vectors for all registered adapters include Anthropic and Gemini |
| `cargo clippy -p codegg-providers --all-targets --locked -- -D warnings` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo check --workspace --all-targets --locked` | Passed |
| `scripts/verify.sh quick` static guards | Schema, boundary, sandbox, execution, TUI, HTTP, audit, scheduler, provider-wire, and Eggwork guards passed |
| `scripts/check_provider_wire_cutover.py` | Passed |
| `cargo fmt --all -- --check`; `git diff --check` | Passed |
| Broad workspace tests | Not green on this rebased baseline: three scheduler cancellation tests fail asserting the executor observes cancellation. The same isolated `cancel_running_job_terminates_process_and_releases_permit` test fails on a detached untouched `origin/main` worktree with the same assertion. No provider test failed. |

No live provider credentials or external provider calls were used. The isolated nextest profile was started for the full workspace but did not complete in this local run; no result is claimed for it.

## 5. Invariant review

Provider IDs, connection kinds, durable storage, model discovery, credentials, and endpoint construction are unchanged. The shared kernel receives only canonical request semantics and provider response bytes. Provider policy remains projected by CodeGG. Tool calls return through canonical IDs/names before agent authorization. Bedrock SigV4/ConverseStream and hosted/stateful Responses remain specialized.

## 6. Failure and recovery review

The stream decoder and tool accumulator are per request. EOF, malformed framing, incomplete tool state, and provider errors remain failures; no success is synthesized. Existing HTTP status and retry/error classification stay outside the shared codec. The workspace cancellation assertion failure was reproduced on the untouched baseline and lies outside this provider change.

## 7. Migration and compatibility review

No storage/config migration or provider-ID change is required. Transcript evidence covers Anthropic assistant text, tools, multiple tool results, and denied results, plus Gemini GenerateContent fields. System instructions are encoded once when they are already present in canonical history; the `ChatRequest.system` field is added only when history lacks a system role. Existing typed Anthropic content blocks are preserved by a bounded post-encode correction.

## 8. Security review

Credentials, API keys, endpoint URLs, and headers remain in CodeGG transport constructors and are never passed into `eggpool-wire`. Reasoning history/output is filtered unless the resolved wire policy explicitly permits it. No dependency or lockfile changes were made.

## 9. Documentation and operations

`architecture/provider.md` and `architecture/model-adapters.md` describe the final shared wire ownership and policy boundary. `scripts/check_provider_wire_cutover.py` checks shared stream adapter use for Anthropic/Gemini and disallows their removed local parser symbols as well as the retired OpenAI parser symbols.

## 10. Unresolved findings

- **Low, pre-existing:** the broad workspace test suite has scheduler cancellation failures on this branch and on untouched `origin/main`. The isolated baseline reproduction confirms no regression from this milestone. No provider correctness or security finding remains open.

## 11. Roadmap disposition

M003 closes the provider wire-kernel consolidation roadmap. All three milestones are closed. Provider-profile metadata deduplication remains explicitly separate and unregistered; no additional provider capability is claimed.

## 12. Registry updates

Dependency audit of `plans/registry.md` and registered subsystem roadmaps found no remaining ready, active, or blocked plan with M003 as a hard or interface dependency. The only downstream plan was M003 itself. The provider wire subsystem moves out of active roadmaps and into recently closed work. The pre-existing scheduler cancellation finding remains owned by its current scheduler/test workstream; this closure does not register duplicate follow-up work.
