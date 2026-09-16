# Execution Reliability, Approval, and Autonomy M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/execution-reliability-approval-autonomy/001-provider-retry-attempt-safety-and-taxonomy.md`

Source subsystem roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Repository baseline reviewed: `76deb2b8`

Implementation commits or pull requests:

- `88694706` — execution-reliability M001: provider retry attempt safety and error taxonomy

## 1. Executive finding

M001 is complete. Provider streaming retries are now attempt-safe with a
correct transient/permanent taxonomy: every logical turn runs a UUID-scoped
attempt chain, only pre-visible transient failures retry, mid-stream
failures after visible output supersede explicitly instead of replaying,
auth/invalid-request/model errors never retry blindly, transport/5xx/429/
timeout classify transient with bounded Retry-After/jitter/cancellation,
and terminal stream outcomes are charged to the existing circuit health.
No provider failover changes session selection, no durable replay was
invented, and no new CI lane or second health service was added.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Retry classification taxonomy (WP-A) | `crates/codegg-providers/src/error.rs::retry_disposition`, `error_class`, `is_transient_transport_kind`, `is_transient_api_error`; `retry_taxonomy_matrix` unit test | pass | Auth/ModelNotFound permanent; CircuitOpen conditional; 429/5xx/timeout/Stream/transient-Transport transient |
| Auth not retried (WP-A) | `is_retryable()` false for `Auth`; `from_http_status` maps 401/403 to `Auth`; `m001_permanent_auth_does_not_retry` (1 call) | pass | Corrects the baseline inversion where Auth retried and transport did not |
| Transport normalization, secret-safe (WP-A) | `From<eggfetch_core::Error>`: Timeout/Connect/Tls/Io/Hyper/pool/proxy-connect/H3/refused-H2 to `Transport{kind}`; InvalidUrl stays `request_error`; all messages are kind-only | pass | `eggfetch_transport_errors_are_secret_safe_and_classified`, `transport_fixture_classifies_transient`, harness secret test |
| HTTP status mapping (WP-A) | `from_http_status()`; `openai_compatible.rs` + `responses_api.rs` + `sse_parser.rs` preserve numeric status; 429 branches use `rate_limit_from_headers()` in all 9 streaming adapters | pass | `http_status_mapping_is_explicit` |
| Retry-After + cap/parsing (WP-A/D) | `parse_retry_after`, `retry_after_from_headers`, `rate_limited`, `MAX_RETRY_AFTER_HINT` (30s); `retry_after_parsing_and_cap` | pass | Delay-seconds only; HTTP-dates intentionally unsupported (documented) |
| Attempt identity + lifecycle (WP-B) | `new_attempt_id()` (UUID-8, per attempt, independent per concurrent turn); `ProviderAttemptStarted/Failed/Superseded` bus events with `attempt_index`, `error_class`, `visible_output`, `will_retry` | pass | Stable for exactly one replay; nested under session/turn |
| Visible-output policy (WP-B) | `stream_once` visible flag over TextDelta/ReasoningDelta/ToolCallStarted; retry loop supersedes + typed `interrupted after visible output (attempt, class)` error; partial buffer discarded | pass | Simplest safe policy per handoff note: no replay after visible output |
| No double tool execution (WP-B) | Discard-on-failure + `m001_midstream_tool_start_is_explicit_and_not_executed` (0 executions, no ToolResult) | pass | Abandoned ToolCallStarted stays visible for attribution only |
| Full-stream provider health (WP-C) | `CircuitBreaker::try_admit()` (shared admission machine, no eager success); `wrap_stream_with_health` charges terminal outcome exactly once; `transient_taxonomy_fails_over_despite_narrow_status_list`, `terminal_stream_failure_is_charged_to_circuit` (3 terminal failures open the breaker) | pass | No second health owner; success_threshold semantics preserved |
| Backoff/jitter/hints/cancellation/diagnostics (WP-D) | `backoff_cap` (exp 1/2/4s, 30s cap, hint raises floor under cap) + `apply_full_jitter` + `sleep_cancellable` + attempt tracing with provider/model/class (no secrets/URLs) | pass | `backoff_cap_grows_and_respects_hint_cap`, `full_jitter_stays_within_bounds` |
| Focused unit tests (§10) | taxonomy table, Retry-After cap/parsing, jitter bounds, attempt-ID uniqueness, non-provider permanent | pass | 6 error tests + 4 provider-turn tests |
| Integration tests (§10) | 8 new `m001_*` harness tests: pre-stream 429/503 recover; mid-stream text/tool explicit; 400/auth/model-missing single-call; secret redaction | pass | 8/8 pass; full harness 48/48 pass |
| Cancellation/contention (§10) | cancel checked before each attempt, each stream event, and during backoff (`sleep_cancellable`); concurrent turns get independent UUID-8 IDs (`attempt_ids_are_unique_per_attempt`) | pass | Backoff-cancel returns `provider turn cancelled` with no further attempt; no dedicated timing-flaky cancel test added (see §10) |
| Security/negative (§10) | Secret-safe mapping tests + harness `m001_secret_bearing_transport_stays_redacted_and_permanent`; 400 loop cannot exhaust (1 call) | pass | Attempt diagnostics carry class/IDs only |
| Migration/compat (§10) | No migration; bus additions map to `None` in `map_app_event_to_core_event`; `RateLimit` display string unchanged; `exec.rs`/`error.rs` status mappings extended | pass | Older clients ignore new bus variants |

## 3. Production implementation evidence

Landed ownership and behavior:

```text
ProviderError taxonomy (codegg-providers/src/error.rs)
  Permanent: Auth, ModelNotFound, NotFound, invalid-request/policy Api,
             permanent Transport (config/validation kinds)
  Transient: RateLimit/RateLimited, Timeout, Stream, transient Transport,
             transient Api (408/425/429/500/502/503/504)
  Conditional: CircuitOpen (turn loop never retries; only probe/refresh paths)

Attempt loop (src/agent/provider_turn.rs, max 3 attempts)
  attempt -> Started event -> stream_once (visible flag)
    ok -> return events
    err + cancelled -> cancelled error, no retry
    err + visible -> Superseded + Failed(will_retry=false) + interrupted error
    err + transient + budget -> Failed(will_retry=true) + jittered hint-aware sleep -> next attempt
    err + permanent/conditional/exhausted -> Failed(will_retry=false) + original error

Fallback health (fallback.rs + circuit.rs)
  try_admit() admission only -> provider.stream()
    acquisition err -> record_failure once -> status-list OR taxonomy failover
    stream ok -> wrap_stream_with_health -> clean EOF records success,
                terminal stream err records one failure
```

Pre-stream failure retries and succeeds (429/503 harness proofs); mid-stream
visible failure never merges generations (text/tool harness proofs);
429/503 retry while auth/400/model-missing stop after one call; transport
connect/TLS/IO fixtures classify transient; invalid-request loops cannot
exhaust. No session provider/model failover occurs: the same provider
object and request are reused; `FallbackProvider` inner failover is the
pre-existing acquisition policy, unchanged in authority.

Deliberately absent (out of scope): global nested retry budget (M002),
tool-side retries, approval/sandbox work, provider resume tokens, durable
provider-call replay, H2 GOAWAY retryability, HTTP-date Retry-After.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-providers
cargo test --test agent_loop_harness -- retry
cargo test --test agent_loop_harness -- stream
cargo test --test agent_loop_harness -- m001
cargo test -p codegg --lib agent::provider_turn
cargo test --test agent_loop_harness
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Focused target names (`m001`, `retry`, `stream`, `agent::provider_turn`)
are the current names; recorded here per plan §11.

### Results

- `cargo test -p codegg-providers`: 136/136 pass (incl. 6 taxonomy/mapping
  tests, fallback failover + circuit-charging tests).
- `cargo test --test agent_loop_harness -- retry`: 4/4 pass.
- `cargo test --test agent_loop_harness -- stream`: 4/4 pass.
- `cargo test --test agent_loop_harness -- m001`: 8/8 pass (new).
- `cargo test -p codegg --lib agent::provider_turn`: 4/4 pass (new).
- `cargo test --test agent_loop_harness` (full): 48/48 pass.
- `cargo test -p codegg --lib error`: 146 selected pass (status mapping
  incl. RateLimited/Transport).
- `cargo test -p codegg-core --lib bus`: 4/4 pass.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass (one
  `useless_format` + six test `unnecessary_to_owned` findings fixed).
- `scripts/verify.sh quick`: pass (fmt, builtin-agents check,
  core-boundary, sandbox, execution-ownership, tui-authority, workspace
  check).

Pre-stream/mid-stream scripted traces with attempt IDs: the `m001_*`
harness tests assert `ProviderAttemptStarted`/`Superseded`/`Failed` carry
one shared attempt ID per failed turn, `visible_output=true`,
`will_retry=false`, class `stream_interrupted`, and exactly one visible
delta generation. Retry/backoff/cancellation evidence: unit bounds tests
plus pre-stream recovery tests; backoff cancel path returns the typed
cancelled error (no timing-flaky dedicated test; see §10).

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Auth not retried without credential refresh | `Auth` permanent; 401/403 map to `Auth`; harness single-call proof; no refresh path exists in the turn loop |
| Invalid request/model/policy permanent | 400/404/422 + unknown codes permanent; model-missing/model-not-found single-call proofs |
| 429/temporary server/transport/timeout retryable in bounds | Taxonomy + max-3-attempts + 30s cap + jitter; 429/503 recovery proofs; transport fixture proofs |
| Visible output attributable; no masquerading generations | Attempt IDs on Started/Failed/Superseded; no-replay policy; single-generation delta assertion |
| No double tool execution via retry | Partial buffer discarded; 0-execution + no-ToolResult proof |
| Credentials/URLs redacted | Kind-only transport messages; header-only Retry-After; class/ID-only diagnostics; secret-URL test |
| Cancellation stops backoff/retry promptly | Checks before attempt, per event, and in cancellable sleep; cancelled error, no further attempt |
| Session provider/model unchanged by retry | Same handle/request reused; only diagnostic reads of name/model |

## 6. Failure and recovery review

- Duplicate delivery: mid-stream abandonment discards partials; terminal
  health records exactly once per attempt (`try_admit` + wrapper contract);
  health updates are slot-scoped, no cross-provider double charge.
- Cancellation races: half-open probe claiming stays inside the single
  write-lock critical section (refactored, not duplicated); `call()`
  behavior preserved (circuit unit tests pass, incl. half-open
  single-probe and timeout-recovery).
- Restart: no durable provider-call replay invented; unfinished turns do
  not resume automatically (existing turn-recovery contract untouched).
- Partial persistence: usage-record insert stays best-effort spawned task
  on success path only; abandoned attempts insert nothing.
- Stale generation: supersession event marks the abandoned generation;
  later attempts carry new IDs and never append to the old stream.
- Contention: concurrent turns generate independent IDs; fallback
  admission stays atomic; no new shared mutable retry state.
- Malformed input: unparsable Retry-After yields `None` (exponential
  fallback); unknown API codes default permanent (conservative).
- Bounded events: attempt events carry IDs/class/flags only; error
  previews truncated at 500 chars in `from_http_status`.

## 7. Migration and compatibility review

- No DB migration; no storage layout change.
- Bus additions are additive variants; `map_app_event_to_core_event`
  maps them to `None` (diagnostic-only), so existing remote/TUI clients
  are unaffected. `event_type()` strings added for new variants.
- `ProviderError` display strings preserved (`rate limit exceeded`
  identical for both rate variants; transport format is kind-only).
- `RateLimit` unit variant retained; `RateLimited`/`Transport` are
  additive. `is_retryable()` semantics intentionally corrected
  (Auth/CircuitOpen no longer retryable) — the only behavioral break,
  required by the milestone.
- HTTP status mapping in `src/error.rs`/`src/exec.rs` extended for the
  new variants (429 stays 429; Transport maps to 502 like Stream).
- Rollback: reverting `88694706` restores old retry loop; new bus
  variants vanish with it (no persisted data depends on them).

## 8. Security review

- Secret handling: eggfetch URL-bearing errors never cross the boundary;
  only `error.kind()` survives. `from_http_status` truncates bodies and
  never attaches URLs. Tests assert secret API keys and `key=` query
  material absent from final errors.
- Authorization: no auth bypass; turn loop performs no credential
  refresh and never escalates on `Auth`.
- Privilege: no provider/model failover beyond pre-existing fallback
  acquisition; session selection untouched.
- DoS bounds: 3 attempts, 30s delay cap, 30s Retry-After clamp, 120s
  setup / 90s idle timeouts, jitter against thundering herd.
- Redaction/audit: attempt lifecycle events expose class/IDs/flags only;
  full error text stays in local tracing at warn level with the same
  secret-safe construction.

## 9. Documentation and operations

Updated:

- `architecture/provider.md` — retry taxonomy, transport/Retry-After
  contract, stream-aware fallback health.
- `architecture/agent.md` — provider-turn attempt-safety lifecycle
  (attempts, visible-output gate, backoff/cancel, no protocol change).
- `architecture/error.md` — new variants, corrected `is_retryable`,
  conversion-row and status-table updates.

Operator notes: watch `provider attempt failed before visible output;
retrying` (info, with `delay_ms`/`attempts_left`) vs `interrupted after
visible output; superseded without replay` (warn) vs `failed terminally`
(warn); `fallback: provider … terminal stream failure charged to circuit`
(warn) for health attribution. Attempt IDs correlate Started/Failed/
Superseded triples per turn.

No new static guard was added: the taxonomy is enforced by unit tests
and the visible-output gate by harness tests; a source-level lint would
not meaningfully constrain future retry call sites (plan §6 allows
tests over broad lint rules here).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | No dedicated timing-based cancel-during-backoff test; cancellation is covered by flag checks and unit-adjacent proofs only | Backoff cancel path could regress silently if `sleep_cancellable` wiring changes | M008 fault-injection qualification should add a deterministic cancel-in-backoff fixture; do not add flaky timing tests here |
| Low | HTTP-date `Retry-After` values are ignored (delay-seconds only) | Servers sending dates get exponential backoff instead of hint-accurate delay | Accept unless provider evidence shows date hints in practice; extend `parse_retry_after` then |
| Low | Fallback inter-provider delay (1s/2s/4s…) is not cancellation-aware (provider layer has no cancel handle) | Cancel during fallback failover waits out one short sleep | Turn-level cancel still stops the outer retry promptly; thread a cancel handle into fallback only if M002/M008 needs it |
| — | No other open items | — | — |

No stop condition triggered (no provider resume protocol required, no
silent connection/model switch, no credential-refresh semantics needed,
no durable replay invented).

## 11. Roadmap disposition

Milestone closed and the hard dependency may proceed:

- M001 (provider retry attempt safety and taxonomy): hard dependency
  satisfied — close.
- M002 (unified retry budget and side-effect reconciliation): was blocked
  on M001 closure — unblock to **ready** (roadmap §6 classifies M002 as
  hard-depending on M001 only).
- M003 remains independently **ready** (parallel track, unaffected).
- M004-M007 remain blocked on the M003 chain (unchanged).
- M008 remains blocked on M001-M007 (M001 leg now satisfied; still
  blocked on the rest).

## 12. Registry updates

- `plans/registry.md`: M001 `ready` → `closed` with closure link and
  implementation commit `88694706`; subsystem row current milestone
  `M001 and M003 ready` → `M001 closed; M003 ready`; dependency-ready
  table M001 row → `closed`; blocked-work M002 row `M001 provider retry
  closure` → unblocked/`ready` (handoff: M002 unified retry budget);
  execution-order item 2 rewritten to reflect M001 closure + M002 ready;
  M001 appended to recently-closed work.
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`:
  M001 section `ready` → `closed` with closure link; M002 `blocked on
  M001` → `ready`; status table updated.
- `plans/implementation/execution-reliability-approval-autonomy/001-provider-retry-attempt-safety-and-taxonomy.md`:
  `Status: ready for handoff` → `Status: implemented`.
