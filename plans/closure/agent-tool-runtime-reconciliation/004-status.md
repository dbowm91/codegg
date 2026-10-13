# Agent Tool Runtime Reconciliation M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-tool-runtime-reconciliation/004-provider-deferral-wire-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md#6-dependencies-and-milestones`

Repository baseline reviewed: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`
(plus accepted M001–M003 closures on this branch, HEAD `832205c7`;
no drift beyond them).

Hard dependency: M002 accepted closure
`plans/closure/agent-tool-runtime-reconciliation/002-status.md`
(verified before code change; deferred catalog identity complete).
Soft dependency: M003 accepted closure
`plans/closure/agent-tool-runtime-reconciliation/003-status.md`
(security-review input; satisfied — descendant ceilings hold while
disclosure was qualified).

Implementation commits or pull requests:

- Pending commit on `planning/agent-tool-runtime-reconciliation` —
  M004 disclosure qualification (same push as M003; files below; SHA
  to be recorded at commit time)
  - `crates/codegg-providers/src/provider_core.rs` —
    `ToolDisclosureWireMode`, `qualify_tool_disclosure[_with_allowlist]`,
    empty native allowlists, lib re-exports, 3 matrix tests
  - `crates/codegg-providers/src/responses_api.rs` —
    `ResponsesTool.defer_loading` propagation +
    `native_tool_search_entry` (exactly `{"type":"tool_search"}`),
    2 DTO tests
  - `crates/codegg-providers/src/wire.rs` —
    `ANTHROPIC_NATIVE_TOOL_SEARCH_VARIANT`,
    `is_provider_tool_search_entry`,
    `apply_native_openai/anthropic_tool_search` (gated emitters),
    `disclosure_tests` census + native + stream fixtures (8 tests)
  - `crates/codegg-providers/src/lib.rs` — new-type re-exports
  - `src/agent/request_preparation.rs` — per-turn qualified mode +
    telemetry on cache-hit and uncached paths (partition/payload
    behavior byte-identical)
  - `tests/provider_tool_disclosure_wire.rs` (new) — 10 byte-level
    cross-surface tests (shapes, matrix, downgrade, 0/5/40/100,
    measured tradeoffs, DTO parity)
  - `scripts/check_tool_disclosure_wire.py` (new) — static guard
  - `architecture/provider.md`, `architecture/tool.md`,
    `architecture/agent-tool-surface.md`,
    `crates/codegg-config/src/schema.rs` (`ToolDeferralConfig`
    wire-honesty comment) — disclosure story reconciled

## 1. Executive finding

M004 is complete as an infrastructure boundary. Tool deferral is now
an **honest, measured wire behavior**: the baseline census (pinned
`eggpool-wire` 0.1.0) proves generic Chat **drops** `defer_loading`
while sending full schemas (advertised deferral was a no-op there),
Responses/Messages preserve the flags but emit no provider search
entry (inert without one), and unknown provider stream items decode to
nothing executable. `ToolDisclosureWireMode` replaces generic-ID
inference with exact provider/model qualification; both native
allowlists are empty (no live proof exists), so all production traffic
is `FullCompatibility` with byte-identical payloads to baseline, and
telemetry says so every turn. Native emitters are implemented,
fixture-proven (literal shapes, two-turn discover→call, idempotent
builtin, typed downgrade, no invented tool results), and gated. No
live-provider support is claimed for any combination. No ADR: provider
transports, EggPool profile ownership, and the hosted Responses
program subsystem are untouched (the direct bridge still never routes
through it — enforced by the multi-surface guard that caught one
violation during this work).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A — Honest capability/wire matrix over OpenAI Responses, Anthropic Messages, OpenAI-compatible Chat, EggPool/Go multi-surface, aliases; qualified `ToolDisclosureWireMode` (Native×2, FullCompatibility, optional HostStaged only when proven); matrix keyed on wire surface/model/API version; unknown → FullCompatibility; no EggPool contract change; no per-turn probing; existing deferral settings honored without pretending support | `ToolDisclosureWireMode` + `qualify_*` (exact provider+model match; `unqualified_provider`/`unqualified_model` reasons); request prep computes mode per turn; partition/palette/`always_loaded`/`max_initial_tools`/disable/role path byte-identical; EggPool untouched | pass | HostStaged not introduced (no proven continuation semantics — explicit non-goal). Surface string is dispatch-time knowledge; qualify inputs (provider+model) are logged per turn; per-request surface attribution stays at encode (bounded gap, §10). |
| B — OpenAI Responses native slice (feature-gated): deferred functions with flags in the documented representation + distinct `{"type":"tool_search"}` builtin; model/count/schema/strict limits + call/results parse on recorded fixtures; builtin never via ToolRegistry nor confused with host function; hosted-result handling per native protocol or mode stays disabled (HTTP 200 ≠ done) | `ResponsesTool.defer_loading` propagation; `native_tool_search_entry`; `apply_native_openai_tool_search` (flags enforced, host function removed, builtin exactly once, idempotent, typed error without tools array); two-turn fixture; unknown-item stream fixture proves no execution | pass | Deferred defs are sent flagged (vendor contract), not removed. Hosted results: kernel fails unknown items closed — mode disabled until preservable (§10). |
| C — Anthropic native slice (feature-gated): full defs every request + `defer_loading:true` on deferables + correct variant for eligible model/API; search tool itself immediate; server events/results/`tool_reference` preserved across stream/history/replay/compaction; never synthesize client `tool_result` for server IDs; cache-control valid with deferred defs; fixture-first two-turn proof before enabling | `apply_native_anthropic_tool_search` (full retention, flags, immediate search, variant exactly once, idempotent); variant shape fixture-qualified; stream fixture (no invented results); cache-control negative (no markers emitted) | pass | Variant entry shape is fixture-qualified from vendor docs, NOT live-confirmed — stated in code + here; allowlist empty so it cannot emit in production. History/replay/compaction of server events unproven live (same gate). |
| D — Generic compatibility + optional host staging: OpenAI-compatible Chat, local models, unqualified providers send complete authorized defs + ordinary CodeGG `tool_search` call, logged as full compatibility; host-staged mode only with proven re-request semantics (else separate milestone); keyword lookup alone never makes undisclosed tools callable | Chat census (no native keys, no elision, host search present as function); qualify maps compat/local/Go-selected surfaces to FullCompatibility; no HostStaged mode introduced | pass | Model re-request semantics for staged visibility remain unproven → out of scope by design. |
| E — Diagnostics/docs/safeguards: update tool/surface/provider docs + config comments; per-turn mode, immediate/deferred counts, definition bytes, provider surface, fallback reason, caching; no credential leaks; static checks for native-required builtins and unknown-provider exclusion | Per-turn debug on hit (`cache_hit=true`) + uncached (counts + schema bytes) paths; docs + config comment; `check_tool_disclosure_wire.py` | pass | Surface attribution gap noted above; no secrets in telemetry (names/counts/reasons/fps only). |
| Work packages 1–5 — census, matrix/fallback, OpenAI slice, Anthropic slice, regression/benchmark | Census fixtures ×3 + kernel-notice evidence; matrix + downgrade tests; native slices ×2 + idempotency; 0/5/40/100 scale + 40-tool benchmark (§4) | pass | Live credentials/models unavailable → infrastructure-fixture closure with native gated (plan §8 explicitly permits). |
| Acceptance — accurate per-wire behavior; no generic unsupported native claim; all fallbacks work with policy-eligible tools; enabled natives have fixtures + two-turn + guarded versions (none enabled — explicitly reported); measured tradeoffs; no implied live evidence | Below (§4) + empty allowlists + guard + this record | pass | Zero native combinations enabled; the "enabled native" clause is vacuously satisfied and explicitly reported as such. |

## 3. Production implementation evidence

Baseline census (pinned `eggpool-wire` 0.1.0, fixtures pin it):

- Chat (`encode_openai_chat`, used by direct OpenAI, Azure,
  OpenRouter, Zen, generic compatible, and Go-selected chat): tools
  encode as `{"type":"function","function":{name,description,
  parameters}}` — **no `defer_loading` key**; the kernel files a
  `provider_extension_not_representable` adaptation notice and encodes
  anyway. Full schemas always sent.
- Responses (`encode_openai_responses`, Go-selected surface only —
  direct OpenAI uses Chat): `{"type":"function",...,"defer_loading":
  true}` preserved when set; **no `tool_search` entry of any kind**
  (CodeGG only creates `Function`-kind tools; the kernel's
  `DeferredSearch` client entry is never constructed).
- Messages (`encode_anthropic_messages`, direct Anthropic +
  Go-selected): `defer_loading` value preserved per tool; **no search
  variant**; no cache-control markers emitted.
- Streams: unknown output items (provider search–shaped) decode to
  nothing (`_ => {}`) or a typed `provider_stream` error — the
  fixture proves a search-shaped item poisons nothing executable and
  invents no `ToolResult` (observed: kernel fails that synthetic
  stream closed; nothing executes either way).

What changed beyond the census:

- Qualification is pure, exact-match, stateless (recomputed per
  turn/call — no sticky cache; model-switch/failover re-evaluate).
- Preparation still partitions exactly as before (palette/discovery/
  cache/sort behavior preserved — `agent_loop_harness` 52/52,
  M002/M003 suites green); the mode only adds honest telemetry.
- Native emitters live in `wire.rs` (direct bridge; no hosted
  subsystem routing — multi-surface guard green after one caught
  violation), operate on already-encoded bodies, and are reachable in
  production only through the (empty) allowlists.
- `ResponsesTool` gains the optional flag (absent when unset:
  compatibility DTOs byte-stable) plus the single builtin
  constructor (guard-pinned to one production site).

What did not change: EggPool profile/contract and Go dispatch;
hosted Responses program subsystem; provider selection/rotation;
retry/failover (no cross-wire `tool_reference` replay — nothing
produces them); storage schema (none); config keys (comment only);
user-visible call semantics (FullCompatibility bytes identical).

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers
cargo test -p codegg --lib -- agent::request_preparation tool::tool_search tool::disclosure
cargo test -p codegg --test provider_tool_disclosure_wire
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test tool_surface_minimization
cargo test -p codegg --test agent_tool_m002_discovery
cargo test -p codegg --test agent_tool_m003_descendant
cargo test -p codegg --test causal_observe_replay
cargo test -p codegg --test eggsact_deterministic_tools
cargo test -p codegg --test tool_registry
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_tool_disclosure_wire.py
bash scripts/verify.sh quick
```

### Results

- `cargo fmt --all -- --check`: exit 0.
- `codegg-providers`: 272/272 pass (incl. 3 qualify matrix, 2 DTO,
  8 wire disclosure/census/stream fixtures).
- Root lib sweep (preparation/search/disclosure): 32/32 pass.
- `provider_tool_disclosure_wire`: 10/10 pass (3 shapes, 2 native
  two-turn, downgrade, matrix, 0/5/40/100 scale, 40-tool tradeoffs,
  DTO parity).
- `agent_loop_harness` 52/52, `tool_surface_minimization` 13/13,
  `agent_tool_m002_discovery` 12/12, `agent_tool_m003_descendant`
  20/20, `causal_observe_replay` 13/13,
  `eggsact_deterministic_tools` 26/26, `tool_registry` 14/14.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0
  (one `nonminimal_bool` fixed during work).
- `check_tool_disclosure_wire.py`: `OK` exit 0.
- `verify.sh quick`: exit 0 (after fixing the one real violation it
  exists to catch — direct bridge referencing the hosted subsystem).

Measured tradeoffs (40 mixed native/plugin/MCP tools, varying
schemas — `measured_tradeoffs_40_tools`, eprinted):

- `chat_full=19695 responses_full=20405 messages_full=19165`
  bytes vs `compact_top10=755` bytes (~96% smaller).
- Deferred schemas alone: `12120` bytes (~60% of the full array) —
  the prize a qualified native mode could address; no universal
  savings promised (scales with schema size, not tool count alone).
- Time-to-first-executable-tool and provider cache impact are not
  measurable without live models (no network in this environment) —
  explicitly not claimed.

Not run (recorded, not implied): live provider credentials/models
(any surface), real MCP servers, `lsp-test-support` /
`server,plugins` feature tests beyond quick, full workspace nextest
sweep, native macOS/Windows, hosted CI. Plan §8 literal selectors
(`--workspace tool_deferral/tool_search/provider`) do not exist as
package names; the replacement (providers suite + lib filters +
named integration targets above) is recorded per plan §8.

## 5. Invariant review

- One immutable `ResolvedToolSurface` owns all identities (eager,
  deferred, host-discoverable): holds — M004 changes no identity
  path (M002/M003 suites green); deferral/visibility reduction stay
  named separately from provider deferral.
- No deferred callable the host cannot resolve/authorize: holds —
  FullCompatibility sends exactly the authorized array; native paths
  (gated) keep full definitions + flags.
- Provider references are not grants; retry/failover recomputes
  mode, never replays references: holds — no reference objects are
  produced anywhere (grep-clean); qualify is stateless per call.
- Mode uses explicit wire/model qualification, never generic ID:
  holds — exact-match + empty allowlists + guard; aliases cannot
  enable native (matrix-tested).
- Unqualified → full array + ordinary `tool_search`, never elision
  or fake schemas: holds — census fixtures pin it per surface.

## 6. Failure and recovery review

- No storage migration; DTO additions are optional/skip-empty
  (byte-stable compatibility shapes).
- Network failure / unsupported builtin → typed error, bounded
  handling, no unbounded retry or implicit provider switch (native
  errors are qualification failures → FullCompatibility fallback, not
  transport retries; existing retry/circuit behavior untouched).
- Mid-conversation fallback: FullCompatibility is the only active
  mode, so no mid-conversation mode change exists today; native
  enablement must preserve canonical outcomes or stop with a typed
  error (design constraint recorded for the enabling change).
- Cancellation propagates through existing provider request control
  (untouched). Restart rebuilds mode from pinned provider/model
  config per turn (no persisted speculative support).
- In-flight authority unaffected (M003 snapshot/grant paths
  untouched and green).

## 7. Migration and compatibility review

No migration, no protocol/DTO break (all additions optional or new
gated fns), no config change (comment only), no model-visible rename.
`supports_defer_loading` retained for palette partitioning (no
consumer removed). Unknown providers/aliases behave exactly as
before, now honestly logged. Rollback is a plain revert.

## 8. Security review

- Disclosure never widens authority (M002/M003 ownership intact;
  native emitters only reshape advertisement of already-authorized
  tools; host `tool_search` removal is advertisement-only).
- No provider accepts a deferred callable the host cannot
  resolve — FullCompatibility arrays are 1:1 with the authorized
  surface (per-tool schema equality asserted).
- No synthesized client `tool_result` for provider-owned IDs
  (stream fixture + hosted-adapter `HostedTool` arm both refuse).
- Credentials/PII: telemetry carries provider/model/mode/reason/
  counts/bytes only.
- Downgrade abuse (wrong API version, unknown provider, invalid
  model): deterministic FullCompatibility, no retryable-error
  confusion (typed `native_tool_search` API error stays local).

## 9. Documentation and operations

- Updated: `architecture/provider.md` (disclosure matrix + census +
  measurements + guard), `architecture/tool.md` (wire-qualification
  note), `architecture/agent-tool-surface.md` (wire-honesty note),
  `ToolDeferralConfig` (host-only scope comment).
- New guard: `scripts/check_tool_disclosure_wire.py` (manual;
  wiring into `verify.sh`/CI deferred with the other three guards).
- Operator telemetry: `selected tool disclosure wire mode`
  (provider, model, mode, reason, cache_hit, immediate/deferred
  counts, schema bytes) on every turn, both cache paths.
- Recovery: no new failure modes in production (FullCompatibility
  is today's behavior, now logged); native errors are local
  qualification failures with explicit reasons.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No live provider evidence for any surface (no credentials, no models, no network) | Deterministic-fixture closure only; both native modes empty-gated | Any native enablement needs live byte-level + two-turn proof per model/API + allowlist entry in its own change. |
| low | Anthropic variant entry shape is fixture-qualified from docs, not live-confirmed | Wrong-shape risk if enabled as-is | Live-confirm the variant shape before any allowlist entry. |
| low | Hosted search-result preservation (server tool use/result/reference across history/replay/compaction) unproven | Kernel fails unknown items closed today — safe but native-incompatible | A native-enabling change must prove round-trip or keep the mode off (plan §5B/C). |
| low | Per-request wire-surface attribution is not in turn telemetry (surface resolves at dispatch) | Operators see qualify inputs (provider+model), not the Go-selected surface | A future change may log the resolved surface at encode; no inference added meanwhile. |
| low | Time-to-first-tool and provider cache impact unmeasured (need live models) | Benchmark is bytes-only | Live benchmark when credentials exist; no savings promised. |
| low | Host-staged mode not introduced | Token pressure still addressed only by palette + compact search | Separate milestone + ADR if continuation semantics are ever needed (plan §5D). |
| low | New guard not wired into `verify.sh`/CI (now 4 manual guards) | Manual runs only | Wire all four guards into `verify.sh quick` + CI `verify` job as one follow-up. |
| low | Hosted CI + native macOS/Windows not run | Same operational limitation as M001–M003 | Hosted `CI / verify` + Desktop E2E on the closure revision before merge. |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone M004 closed. The agent-tool-runtime-reconciliation roadmap
is complete on its four milestones (M001–M004 closed); roadmap closure
itself follows the planning lifecycle (registry + roadmap status
updates below — no further implementation plans remain in this
subsystem).

No other registered plan is unblocked by this closure.

## 12. Registry updates

- `plans/registry.md`: move M004 `ready` → `closed` (link this
  closure).
- `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md`:
  header `M001–M003 closed; M004 ready` → `M001–M004 closed`;
  milestone table M004 `ready` → `closed`.
- `plans/implementation/agent-tool-runtime-reconciliation/004-provider-deferral-wire-qualification.md`:
  `ready for handoff` → `implemented; closed` with closure link.
