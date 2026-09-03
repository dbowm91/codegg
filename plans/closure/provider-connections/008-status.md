# Provider Connections Milestone 008 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/provider-connections/008-opencode-go-session-header-corrective-pass.md`

Source subsystem roadmap: `plans/subsystems/provider-opencode-session-affinity-corrective-addendum.md`

Repository baseline reviewed: `fca5b5278873c12ea5f2d5ca15a24247d4bf019b`

Implementation commit: `328c26cb8dfe05fbd98092280ba25679af55efa6` — typed
provider request context, OpenCode Go session-affinity transport, static header
application, propagation updates, wire tests, and documentation.

## 1. Executive finding

M008 is strictly closed. CodeGG now projects the canonical session identity into
a bounded `ProviderRequestContext`; the OpenCode Go factory explicitly enables
the required `x-opencode-session` policy; and the OpenAI-compatible transport
emits validated static `extra_headers` without allowing collisions with
transport-owned headers. The request body, provider storage, credentials,
fallback ownership, and public protocol remain unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Typed bounded request context | `ProviderRequestContext` in `provider_core.rs`; all `ChatRequest` constructors compile with explicit context | pass |
| Canonical turn propagation | `DefaultTurnRuntime` validates `SessionId` and attaches it; `AgentLoop` re-projects its canonical session for direct callers and continuations | pass |
| Stable same-session/different-session affinity | Local TCP wire-capture test sends S1, S1, and S2 and asserts exactly one header each | pass |
| Session metadata stays out of model JSON | Same wire test asserts captured bodies contain neither S1 nor S2 | pass |
| Missing context is local and non-retryable | Required-policy test receives `missing_session_context` and fake server receives zero requests; `ProviderError::Api` is non-retryable | pass |
| Invalid header input is rejected | Static invalid name/value tests and typed dynamic policy parsing return `invalid_header` before send | pass |
| OpenCode-only isolation | Normal OpenAI-compatible provider wire test asserts no `x-opencode-session` header | pass |
| `extra_headers` execution and ownership | Wire test observes `Editor-Version`; auth/content/session collisions and duplicate static names fail with `reserved_header_collision` | pass |
| Retry/fallback preservation | `FallbackProvider` passes the same `&ChatRequest` unchanged; `ProviderTurnAdapter` retries the same request reference; agent harness asserts repeated requests retain one context value | pass |
| Security/privacy boundary | No inbound-header passthrough or arbitrary request header map added; logs report no raw session value; credentials remain unchanged | pass |
| Storage/protocol/migration compatibility | No schema, migration, provider-connection, credential, or public protocol changes | pass |
| Documentation | `architecture/provider.md` documents context, OpenCode policy, body exclusion, and static-header ownership | pass |

## 3. Production implementation evidence

The provider boundary now contains only optional canonical session metadata, not
arbitrary headers. `OpenAiCompatibleProvider` owns typed header construction,
validates authentication/static/dynamic values before network I/O, rejects
case-insensitive reserved collisions, and attaches one configured affinity
header. Only `create_opencode_go()` configures `x-opencode-session`.

The turn runtime validates the existing CodeGG `SessionId` contract before
creating the request. `exec` establishes and reuses one invocation-scoped ID
when no session was supplied. Direct `AgentLoop` callers are normalized from
the loop's canonical session, so history compaction, tool continuations,
steering, retries, and fallback do not replace the context.

All remaining production constructors were deliberately classified: compaction,
review, commit, research, and CLI diagnostic requests remain explicit
standalone/default context; subagents use their established subagent session;
`exec` uses its stable invocation ID; and the daemon turn path uses the
canonical daemon session. No inbound OpenCode header handling existed and none
was added.

## 4. Verification executed

All results below are local verification.

```text
cargo test -p codegg-providers -- --test-threads=1
102 passed (2 suites)

cargo test -p codegg-providers openai_compatible -- --test-threads=1
8 passed

PKG_CONFIG_PATH=/usr/local/lib/pkgconfig cargo test --test agent_loop_harness test_agent_loop_harness_records_requests -- --test-threads=1
1 passed

cargo check --workspace --all-targets --locked
passed

cargo clippy -p codegg-providers --all-targets -- -D warnings
passed

cargo fmt --all -- --check
passed

git diff --check
passed

scripts/verify.sh quick
passed
```

The agent-harness test initially encountered the host's default `/opt/local`
arm64 liblzma selection while linking an x86_64 target. Re-running with the
available `/usr/local` x86_64 pkg-config path passed; this was an environment
selection issue, not a source failure. No live OpenCode Go request was made.

## 5. Invariant review

- The provider reads only `ChatRequest.context.session_id`; it never generates
  a random per-request identity or stores session state globally.
- The same immutable request context is reused by retries and fallback; a
  provider without affinity policy ignores it.
- Session identity is transport-only and absent from request JSON, prompts,
  tool data, persistence, and ordinary log values.
- Static configured headers remain provider-owned and cannot override
  authorization, content type, dynamic session affinity, or one another.
- Authorization handling and `Content-Type` ownership remain explicit.

## 6. Failure and recovery review

Missing/invalid context and invalid/colliding configured headers fail before
network I/O with non-retryable `ProviderError::Api` codes. Existing retry and
circuit-breaker behavior is unchanged for network/provider failures. No new
restart or persistence behavior was introduced; restored CodeGG sessions
re-project their existing identity when a request is rebuilt.

## 7. Migration and compatibility review

No database migration, protocol version, provider-connection record, model
catalog, credential, or storage change was made. OpenAI-compatible request JSON
and SSE parsing are unchanged. Existing Copilot `Editor-Version` configuration
now reaches the wire as its declared contract requires.

## 8. Security review

Header names and values are parsed through `reqwest`/HTTP typed APIs before
send, including CR/LF rejection. No raw session or authorization header value
is logged. No generic inbound-header forwarding, credential persistence, or
untrusted prompt/tool/frontend header input was introduced.

## 9. Documentation and operations

`architecture/provider.md` records the request-context seam and OpenCode Go
policy. No live-provider test, CI lane, release automation, or operational
credential change was added.

## 10. Unresolved findings

None at critical, high, medium, or low severity within M008 scope.

## 11. Roadmap disposition

The corrective OpenCode Go/session-affinity addendum is closed. Provider M007
remains unchanged as the historical strict closure for its original storage,
lifecycle, migration, and governance scope. M008 is the current strict
disposition only for the newly discovered request-transport defects.

## 12. Registry updates

- M008 moved from active implementation to closed and was removed from the
  dependency-ready handoff table.
- The provider corrective roadmap and subsystem registry row now report M008
  closed.
- The blocked-work audit found no registered provider downstream plan whose
  hard or interface dependency is M008. The only blocked registry item is the
  unrelated supported-Linux Landlock evidence condition, so no future plan was
  unblocked.
- No new corrective plan is required; deferred arbitrary-header/provider-client
  unification work remains unregistered and deferred.
