# Provider Wire-Kernel Consolidation Milestone 002 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/provider-wire-kernel-consolidation/002-openai-family-shared-wire-cutover.md`
Source subsystem roadmap: `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md#milestone-002--openai-family-shared-wire-cutover`
Repository baseline reviewed: `dcea3ea`
Implementation commits: `dcea3ea — cut over the OpenAI family to shared wire; pending closure commit`

## 1. Executive finding

The native OpenAI, Azure OpenAI, OpenRouter, OpenCode Zen, and generic OpenAI-compatible production paths now use the pinned `eggpool-wire` OpenAI Chat encoder and SSE decoder. Their endpoints, credentials, headers, session affinity, discovery, HTTP errors, and idle deadlines remain CodeGG-owned. The one full-workspace test failure is reproduced unchanged on the rebased `origin/main` baseline and is unrelated scheduler cancellation behavior; provider and transcript verification is recorded below.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| OpenAI family uses shared request and stream grammar | Production constructors delegate through `wire::encode_openai_chat` and `wire::openai_chat_stream`; `scripts/check_provider_wire_cutover.py` | Pass |
| Preserve transport/auth/discovery/session affinity | Existing provider and gateway tests; transport remains in named CodeGG wrappers | Pass |
| Preserve aliases, private reasoning policy, and usage | Provider policy projection and wire tests; OpenAI stream usage requested where required | Pass |
| Retire duplicate OpenAI parser | `sse_parser.rs` is Anthropic-only; static guard rejects former OpenAI parser symbols | Pass |
| No storage/config/provider-ID migration | No schema, provider ID, or durable connection changes | Pass |

## 3. Production implementation evidence

`openai.rs`, `openai_compatible.rs`, `azure.rs`, `openrouter.rs`, `opencode_zen.rs`, and compatible provider wrappers route standard Chat Completions serialization/stream parsing through the shared bridge. Explicitly empty tool collections retain their former JSON distinction. Provider-specific omission behavior is kept as bounded CodeGG corrections. Bedrock, Anthropic, Google, and hosted/stateful Responses remain outside M002.

## 4. Verification executed

All commands were run locally with Rust 1.89 on `aarch64-apple-darwin`:

| Command | Result |
|---|---|
| `cargo test -p codegg-providers` | 174 passed |
| `cargo test --test provider_transcripts` | Passed (21 tests) before final empty-tools correction; focused production gateway contract test passed after it |
| `cargo test --test provider_sse_parser` | 3 passed (Anthropic-only parser regression tests) |
| `cargo clippy -p codegg-providers --all-targets -- -D warnings` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| provider cutover/static and repository boundary guards | Passed |
| `cargo test --workspace` | Provider and other earlier targets passed; three `scheduler_cancellation` cases failed with “executor must observe cancellation token” |
| `cargo nextest run --test scheduler_cancellation --locked --profile ci -E 'test(cancel_running_job_terminates_process_and_releases_permit)'` on `origin/main` | Same cancellation failure reproduced on untouched baseline |

The baseline comparison used a detached worktree at `origin/main`. No live provider credentials or external provider calls were used.

## 5. Invariant review

Provider identity and durable connection compatibility are unchanged. Shared wire receives no credentials, endpoint, or session ID. CodeGG retains transport lifecycle, retry/error classification, and session-affinity authority. Tool aliases are normalized before broker handling.

## 6. Failure and recovery review

The shared decoder is request-owned. HTTP errors are classified before stream decode; malformed or incomplete wire events remain stream errors. No retry or cancellation policy moved into the codec. The known workspace cancellation failures predate this change, as established by baseline reproduction.

## 7. Migration and compatibility review

No persistent migration is required. Historical request-body differences and optional field omissions are covered by focused builder tests. The explicit empty-tools distinction has a regression test.

## 8. Security review

Credentials, endpoint URLs, static headers, and session-affinity values remain in CodeGG transport wrappers and are not passed to the wire kernel. Existing redaction/auth tests pass.

## 9. Documentation and operations

`architecture/provider.md` and `architecture/model-adapters.md` describe the shared OpenAI-family wire owner. `scripts/check_provider_wire_cutover.py` is included in `scripts/verify.sh quick`.

## 10. Unresolved findings

- **Low, pre-existing:** three workspace scheduler cancellation tests fail on both this branch and untouched `origin/main` with the same cancellation-token assertion. This does not affect provider behavior or the M002 acceptance boundary.
- No M002 correctness finding remains open.

## 11. Roadmap disposition

M002 is closed. The shared bridge and pinned kernel contract are stable for the remaining standard provider families.

## 12. Registry updates

M002 moves from closing to recently closed. Dependency audit: M003 was the only registered plan blocked on M002. With M002 closed, its hard dependency is satisfied and the shared decoder/encoder seam is stable; M003 moves to `ready` in the same closure commit. No other registered plan depends on M002.
